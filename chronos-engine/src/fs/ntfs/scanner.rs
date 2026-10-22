use crate::fs::ntfs::attributes::{DataAttributeContent, ATTR_DATA};
use crate::fs::ntfs::bitmap::VolumeBitmap;
use crate::fs::ntfs::mft::{MftParser, MftRecord};
use crate::fs::ntfs::runlist::RunlistDecoder;
use crate::fs::ntfs::usn::UsnJournalParser;
use crate::fs::ntfs::vbr::NtfsVbr;
use crate::hal::device::StorageDevice;
use crate::hal::error::{HalError, HalResult};
use chronos_proto::recovery::{
    DiscoveredFile, FileCategory, FileStatus, HistoryEvent,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub struct NtfsScanner {
    device: Arc<dyn StorageDevice>,
    start_lba: u64,
    vbr: NtfsVbr,
    bitmap: Option<VolumeBitmap>,
}

impl NtfsScanner {
    pub fn new(device: Arc<dyn StorageDevice>, start_lba: u64) -> HalResult<Self> {
        let mut vbr_buf = vec![0u8; 512];
        device.read_sectors(start_lba, 1, &mut vbr_buf)?;

        let vbr = NtfsVbr::parse(&vbr_buf)
            .ok_or_else(|| HalError::IoError("Invalid NTFS Volume Boot Record".into()))?;

        info!(
            cluster_size = vbr.cluster_size,
            mft_cluster = vbr.mft_cluster,
            total_sectors = vbr.total_sectors,
            "NTFS VBR successfully initialized"
        );

        Ok(Self {
            device,
            start_lba,
            vbr,
            bitmap: None,
        })
    }

    /// Convert cluster number to disk byte offset
    fn cluster_to_byte_offset(&self, cluster: u64) -> u64 {
        let sector_size = self.vbr.bytes_per_sector as u64;
        let cluster_bytes = self.vbr.cluster_size as u64;
        (self.start_lba * sector_size) + (cluster * cluster_bytes)
    }

    /// Read an MFT record by its index
    pub fn read_mft_record(&self, record_index: u64) -> HalResult<Option<MftRecord>> {
        let mft_base_offset = self.cluster_to_byte_offset(self.vbr.mft_cluster);
        let record_offset = mft_base_offset + (record_index * self.vbr.mft_record_size as u64);

        let mut record_buf = vec![0u8; self.vbr.mft_record_size as usize];
        self.device.read_at(record_offset, &mut record_buf)?;

        Ok(MftParser::parse_record(&record_buf, record_index))
    }

    /// Initialize the Volume Bitmap from MFT Record 6 ($Bitmap)
    pub fn init_bitmap(&mut self) -> HalResult<()> {
        if let Some(bitmap_record) = self.read_mft_record(6)? {
            for data_attr in bitmap_record.data_attributes {
                match data_attr.content {
                    DataAttributeContent::Resident(bytes) => {
                        let total_clusters = self.vbr.total_sectors / (self.vbr.sectors_per_cluster as u64);
                        self.bitmap = Some(VolumeBitmap::new(bytes, total_clusters));
                        return Ok(());
                    }
                    DataAttributeContent::NonResident { real_size, data_runs, .. } => {
                        let mut bitmap_bytes = vec![0u8; real_size as usize];
                        let mut offset = 0usize;
                        let cluster_size = self.vbr.cluster_size as usize;

                        for run in data_runs {
                            if let Some(lcn) = run.lcn {
                                let run_bytes = (run.cluster_count as usize) * cluster_size;
                                let to_read = run_bytes.min(bitmap_bytes.len().saturating_sub(offset));
                                let disk_offset = self.cluster_to_byte_offset(lcn);
                                self.device.read_at(disk_offset, &mut bitmap_bytes[offset..offset + to_read])?;
                                offset += to_read;
                            }
                        }
                        let total_clusters = self.vbr.total_sectors / (self.vbr.sectors_per_cluster as u64);
                        self.bitmap = Some(VolumeBitmap::new(bitmap_bytes, total_clusters));
                        return Ok(());
                    }
                }
            }
        }
        warn!("Could not locate or parse $Bitmap attribute; defaulting to optimistic recoverability");
        Ok(())
    }

    /// Perform fast scan across MFT records, returning discovered active and deleted files
    pub fn scan_records<F>(
        &self,
        session_id: &str,
        max_records: u64,
        mut on_file_found: F,
    ) -> HalResult<u64>
    where
        F: FnMut(DiscoveredFile),
    {
        let mut total_found = 0u64;

        for record_idx in 0..max_records {
            let record = match self.read_mft_record(record_idx) {
                Ok(Some(r)) => r,
                Ok(None) => continue,
                Err(e) => {
                    debug!("Read MFT record #{} error: {:?}", record_idx, e);
                    continue;
                }
            };

            // Process file names
            for fn_attr in &record.file_names {
                // Ignore DOS short names (namespace == 2) if Win32/POSIX name exists
                if fn_attr.namespace == 2 && record.file_names.len() > 1 {
                    continue;
                }

                let filename = &fn_attr.filename;
                let extension = Path::new(filename)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();

                let category = categorize_extension(&extension);

                let status = if record.is_in_use {
                    FileStatus::FileStatusActive
                } else {
                    FileStatus::FileStatusDeleted
                };

                let mut size = fn_attr.real_size;
                let mut proto_runs = Vec::new();
                let mut score = 100u32;
                let mut start_lba = 0u64;

                // Check $DATA attributes for actual sizes and runlists
                for data_attr in &record.data_attributes {
                    match &data_attr.content {
                        DataAttributeContent::Resident(bytes) => {
                            size = bytes.len() as u64;
                            score = 100;
                        }
                        DataAttributeContent::NonResident { real_size, data_runs, .. } => {
                            size = *real_size;
                            proto_runs = RunlistDecoder::to_proto_runs(data_runs);
                            if let Some(first_run) = data_runs.first() {
                                if let Some(lcn) = first_run.lcn {
                                    start_lba = self.start_lba + (lcn * self.vbr.sectors_per_cluster as u64);
                                }
                            }
                            if let Some(ref bm) = self.bitmap {
                                score = bm.calculate_recoverability_score(data_runs);
                            }
                        }
                    }
                }

                let (created_at, modified_at, accessed_at) = if let Some(ref si) = record.standard_info {
                    (si.created_at, si.modified_at, si.accessed_at)
                } else {
                    (0, 0, 0)
                };

                let discovered = DiscoveredFile {
                    id: format!("mft-{}-{}", record_idx, Uuid::new_v4()),
                    session_id: session_id.to_string(),
                    partition_index: 1,
                    path: format!("/{}", filename),
                    filename: filename.clone(),
                    extension,
                    size_bytes: size,
                    status: status as i32,
                    category: category as i32,
                    mime_type: guess_mime_type(&filename),
                    created_at,
                    modified_at,
                    accessed_at,
                    deleted_at: if !record.is_in_use { modified_at } else { 0 },
                    recoverability_score: score,
                    is_directory: record.is_directory,
                    data_runs: proto_runs,
                    starting_lba: start_lba,
                    md5_hash: String::new(),
                };

                on_file_found(discovered);
                total_found += 1;
            }
        }

        Ok(total_found)
    }
}

use std::path::Path;

fn categorize_extension(ext: &str) -> FileCategory {
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "heic" | "cr2" | "nef" | "arw" => {
            FileCategory::FileCategoryImage
        }
        "mp4" | "mov" | "mkv" | "avi" | "wmv" | "flv" | "webm" | "m4v" => {
            FileCategory::FileCategoryVideo
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" => {
            FileCategory::FileCategoryAudio
        }
        "pdf" | "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "txt" | "csv" | "rtf" => {
            FileCategory::FileCategoryDocument
        }
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "iso" => {
            FileCategory::FileCategoryArchive
        }
        "sqlite" | "db" | "sqlite3" | "mdb" | "accdb" | "sql" => {
            FileCategory::FileCategoryDatabase
        }
        "sys" | "dll" | "exe" | "log" | "ini" | "dat" => {
            FileCategory::FileCategorySystem
        }
        _ => FileCategory::FileCategoryUnknown,
    }
}

fn guess_mime_type(filename: &str) -> String {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".into()
    } else if lower.ends_with(".png") {
        "image/png".into()
    } else if lower.ends_with(".pdf") {
        "application/pdf".into()
    } else if lower.ends_with(".mp4") {
        "video/mp4".into()
    } else if lower.ends_with(".docx") {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into()
    } else if lower.ends_with(".zip") {
        "application/zip".into()
    } else {
        "application/octet-stream".into()
    }
}
