use crate::fs::fat::exfat::{ExFatBpb, ExFatDriver};
use crate::fs::fat::fat32::{Fat32Driver, FatDirectoryEntry};
use crate::fs::fat::vbr::Fat32Bpb;
use crate::hal::device::StorageDevice;
use crate::hal::error::{HalError, HalResult};
use chronos_proto::recovery::{DiscoveredFile, FileCategory, FileStatus};
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

pub enum FatVariant {
    Fat32(Fat32Driver),
    ExFat(ExFatDriver),
}

pub struct FatScanner {
    variant: FatVariant,
    session_id: String,
}

impl FatScanner {
    pub fn new(device: Arc<dyn StorageDevice>, start_lba: u64, session_id: &str) -> HalResult<Self> {
        let mut vbr_buf = vec![0u8; 512];
        device.read_sectors(start_lba, 1, &mut vbr_buf)?;

        if vbr_buf.starts_with(b"\xEB\x76\x90EXFAT ") {
            let bpb = ExFatBpb::parse(&vbr_buf)
                .ok_or_else(|| HalError::IoError("Invalid exFAT Boot Sector".into()))?;
            info!("exFAT filesystem detected at LBA {}", start_lba);
            Ok(Self {
                variant: FatVariant::ExFat(ExFatDriver::new(device, start_lba, bpb)),
                session_id: session_id.to_string(),
            })
        } else if let Some(bpb) = Fat32Bpb::parse(&vbr_buf) {
            info!("FAT32 filesystem detected at LBA {}", start_lba);
            Ok(Self {
                variant: FatVariant::Fat32(Fat32Driver::new(device, start_lba, bpb)),
                session_id: session_id.to_string(),
            })
        } else {
            Err(HalError::IoError("Not a valid FAT32 or exFAT filesystem".into()))
        }
    }

    /// Perform fast scan across root directory and subdirectories discovering active & deleted files
    pub fn scan<F>(&self, mut on_file_found: F) -> HalResult<u64>
    where
        F: FnMut(DiscoveredFile),
    {
        let mut total_found = 0u64;

        match &self.variant {
            FatVariant::Fat32(driver) => {
                let mut queue = VecDeque::new();
                queue.push_back((2u32, "/".to_string()));

                while let Some((cluster, parent_path)) = queue.pop_front() {
                    let entries = match driver.parse_directory_cluster(cluster) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    for entry in entries {
                        if entry.short_name == "." || entry.short_name == ".." {
                            continue;
                        }

                        let filename = entry.long_name.unwrap_or(entry.short_name);
                        let full_path = if parent_path == "/" {
                            format!("/{}", filename)
                        } else {
                            format!("{}/{}", parent_path, filename)
                        };

                        if entry.is_directory && entry.first_cluster >= 2 {
                            queue.push_back((entry.first_cluster, full_path.clone()));
                        }

                        let ext = std::path::Path::new(&filename)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_ascii_lowercase();

                        let status = if entry.is_deleted {
                            FileStatus::FileStatusDeleted
                        } else {
                            FileStatus::FileStatusActive
                        };

                        let runs = driver.build_cluster_chain(entry.first_cluster, entry.file_size).unwrap_or_default();
                        let start_lba = driver.cluster_to_lba(entry.first_cluster);

                        let file = DiscoveredFile {
                            id: format!("fat-{}", Uuid::new_v4()),
                            session_id: self.session_id.clone(),
                            partition_index: 1,
                            path: full_path,
                            filename,
                            extension: ext,
                            size_bytes: entry.file_size as u64,
                            status: status as i32,
                            category: FileCategory::FileCategoryUnknown as i32,
                            mime_type: "application/octet-stream".into(),
                            created_at: entry.created_at,
                            modified_at: entry.modified_at,
                            accessed_at: entry.modified_at,
                            deleted_at: if entry.is_deleted { entry.modified_at } else { 0 },
                            recoverability_score: if entry.is_deleted { 90 } else { 100 },
                            is_directory: entry.is_directory,
                            data_runs: runs,
                            starting_lba: start_lba,
                            md5_hash: String::new(),
                        };

                        on_file_found(file);
                        total_found += 1;
                    }
                }
            }
            FatVariant::ExFat(driver) => {
                let files = driver.parse_directory_set(2)?;
                for mut f in files {
                    f.session_id = self.session_id.clone();
                    on_file_found(f);
                    total_found += 1;
                }
            }
        }

        Ok(total_found)
    }
}
