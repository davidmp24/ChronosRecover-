use crate::hal::device::StorageDevice;
use crate::hal::error::HalResult;
use byteorder::{ByteOrder, LittleEndian};
use chronos_proto::recovery::{DataRun, DiscoveredFile, FileCategory, FileStatus};
use std::sync::Arc;
use uuid::Uuid;

pub const EXFAT_ENTRY_FILE_ACTIVE: u8 = 0x85;
pub const EXFAT_ENTRY_FILE_DELETED: u8 = 0x05;
pub const EXFAT_ENTRY_STREAM_ACTIVE: u8 = 0xC0;
pub const EXFAT_ENTRY_STREAM_DELETED: u8 = 0x40;
pub const EXFAT_ENTRY_NAME_ACTIVE: u8 = 0xC1;
pub const EXFAT_ENTRY_NAME_DELETED: u8 = 0x41;

#[derive(Debug, Clone)]
pub struct ExFatBpb {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub cluster_size: u32,
    pub fat_offset_sectors: u64,
    pub fat_length_sectors: u64,
    pub cluster_heap_offset_sectors: u64,
    pub cluster_count: u32,
    pub root_dir_first_cluster: u32,
    pub volume_length_sectors: u64,
}

impl ExFatBpb {
    pub fn parse(sector_0: &[u8]) -> Option<Self> {
        if sector_0.len() < 512 || !sector_0.starts_with(b"\xEB\x76\x90EXFAT ") {
            return None;
        }

        let bytes_per_sec_shift = sector_0[108];
        let sec_per_clus_shift = sector_0[109];
        let bytes_per_sector = 1u32 << (bytes_per_sec_shift.min(16));
        let sectors_per_cluster = 1u32 << (sec_per_clus_shift.min(16));
        let cluster_size = bytes_per_sector * sectors_per_cluster;

        let fat_offset = LittleEndian::read_u32(&sector_0[80..84]) as u64;
        let fat_length = LittleEndian::read_u32(&sector_0[84..88]) as u64;
        let cluster_heap = LittleEndian::read_u32(&sector_0[88..92]) as u64;
        let cluster_count = LittleEndian::read_u32(&sector_0[92..96]);
        let root_cluster = LittleEndian::read_u32(&sector_0[96..100]);
        let volume_length = LittleEndian::read_u64(&sector_0[72..80]);

        Some(Self {
            bytes_per_sector,
            sectors_per_cluster,
            cluster_size,
            fat_offset_sectors: fat_offset,
            fat_length_sectors: fat_length,
            cluster_heap_offset_sectors: cluster_heap,
            cluster_count,
            root_dir_first_cluster: root_cluster,
            volume_length_sectors: volume_length,
        })
    }
}

pub struct ExFatDriver {
    device: Arc<dyn StorageDevice>,
    start_lba: u64,
    bpb: ExFatBpb,
}

impl ExFatDriver {
    pub fn new(device: Arc<dyn StorageDevice>, start_lba: u64, bpb: ExFatBpb) -> Self {
        Self {
            device,
            start_lba,
            bpb,
        }
    }

    pub fn cluster_to_lba(&self, cluster: u32) -> u64 {
        if cluster < 2 {
            return self.start_lba + self.bpb.cluster_heap_offset_sectors;
        }
        let cluster_offset = (cluster - 2) as u64 * self.bpb.sectors_per_cluster as u64;
        self.start_lba + self.bpb.cluster_heap_offset_sectors + cluster_offset
    }

    /// Traverse directory entry sets in exFAT (groups of 32-byte records)
    pub fn parse_directory_set(
        &self,
        cluster: u32,
    ) -> HalResult<Vec<DiscoveredFile>> {
        let cluster_bytes = self.bpb.cluster_size as usize;
        let lba = self.cluster_to_lba(cluster);
        let mut cluster_buf = vec![0u8; cluster_bytes];
        self.device.read_sectors(lba, self.bpb.sectors_per_cluster as u64, &mut cluster_buf)?;

        let mut files = Vec::new();
        let chunks: Vec<&[u8]> = cluster_buf.chunks_exact(32).collect();
        let mut i = 0;

        while i < chunks.len() {
            let entry = chunks[i];
            let entry_type = entry[0];

            if entry_type == 0x00 {
                // End of directory
                break;
            }

            // Check if File Directory Entry (0x85 active or 0x05 deleted)
            if entry_type == EXFAT_ENTRY_FILE_ACTIVE || entry_type == EXFAT_ENTRY_FILE_DELETED {
                let is_deleted = entry_type == EXFAT_ENTRY_FILE_DELETED;
                let secondary_count = entry[1] as usize;
                let file_attributes = LittleEndian::read_u16(&entry[4..6]);
                let is_dir = (file_attributes & 0x0010) != 0;

                let mut file_size = 0u64;
                let mut first_cluster = 0u32;
                let mut no_fat_chain = false;
                let mut name_u16 = Vec::new();

                // Process secondary entries
                for sec_idx in 1..=secondary_count {
                    if i + sec_idx >= chunks.len() {
                        break;
                    }
                    let sec_entry = chunks[i + sec_idx];
                    let sec_type = sec_entry[0];

                    if sec_type == EXFAT_ENTRY_STREAM_ACTIVE || sec_type == EXFAT_ENTRY_STREAM_DELETED {
                        let flags = sec_entry[1];
                        no_fat_chain = (flags & 0x02) != 0;
                        file_size = LittleEndian::read_u64(&sec_entry[24..32]);
                        first_cluster = LittleEndian::read_u32(&sec_entry[20..24]);
                    } else if sec_type == EXFAT_ENTRY_NAME_ACTIVE || sec_type == EXFAT_ENTRY_NAME_DELETED {
                        // 15 UTF-16 chars from offset 2..32
                        for c in 0..15 {
                            let off = 2 + c * 2;
                            let ch = LittleEndian::read_u16(&sec_entry[off..off + 2]);
                            if ch != 0x0000 {
                                name_u16.push(ch);
                            }
                        }
                    }
                }

                let filename = String::from_utf16_lossy(&name_u16).trim().to_string();
                if !filename.is_empty() {
                    let ext = std::path::Path::new(&filename)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();

                    let status = if is_deleted {
                        FileStatus::FileStatusDeleted
                    } else {
                        FileStatus::FileStatusActive
                    };

                    let cluster_count = ((file_size + self.bpb.cluster_size as u64 - 1) / self.bpb.cluster_size as u64).max(1);
                    let runs = vec![DataRun {
                        cluster_offset: first_cluster as u64,
                        cluster_count,
                    }];

                    files.push(DiscoveredFile {
                        id: format!("exfat-{}", Uuid::new_v4()),
                        session_id: String::new(),
                        partition_index: 1,
                        path: format!("/{}", filename),
                        filename,
                        extension: ext,
                        size_bytes: file_size,
                        status: status as i32,
                        category: FileCategory::FileCategoryUnknown as i32,
                        mime_type: "application/octet-stream".into(),
                        created_at: 0,
                        modified_at: 0,
                        accessed_at: 0,
                        deleted_at: 0,
                        recoverability_score: 100,
                        is_directory: is_dir,
                        data_runs: runs,
                        starting_lba: self.cluster_to_lba(first_cluster),
                        md5_hash: String::new(),
                    });
                }

                i += 1 + secondary_count;
            } else {
                i += 1;
            }
        }

        Ok(files)
    }
}
