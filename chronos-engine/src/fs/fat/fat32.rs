use crate::fs::fat::lfn::LfnBuilder;
use crate::fs::fat::vbr::Fat32Bpb;
use crate::hal::device::StorageDevice;
use crate::hal::error::HalResult;
use byteorder::{ByteOrder, LittleEndian};
use chronos_proto::recovery::{DataRun, DiscoveredFile, FileCategory, FileStatus};
use std::sync::Arc;
use uuid::Uuid;

pub const FAT_ATTR_READ_ONLY: u8 = 0x01;
pub const FAT_ATTR_HIDDEN: u8 = 0x02;
pub const FAT_ATTR_SYSTEM: u8 = 0x04;
pub const FAT_ATTR_VOLUME_ID: u8 = 0x08;
pub const FAT_ATTR_DIRECTORY: u8 = 0x10;
pub const FAT_ATTR_ARCHIVE: u8 = 0x20;
pub const FAT_ATTR_LFN: u8 = 0x0F;

#[derive(Debug, Clone)]
pub struct FatDirectoryEntry {
    pub is_deleted: bool,
    pub is_directory: bool,
    pub short_name: String,
    pub long_name: Option<String>,
    pub first_cluster: u32,
    pub file_size: u32,
    pub created_at: i64,
    pub modified_at: i64,
}

pub struct Fat32Driver {
    device: Arc<dyn StorageDevice>,
    start_lba: u64,
    bpb: Fat32Bpb,
}

impl Fat32Driver {
    pub fn new(device: Arc<dyn StorageDevice>, start_lba: u64, bpb: Fat32Bpb) -> Self {
        Self {
            device,
            start_lba,
            bpb,
        }
    }

    /// Convert cluster number to LBA
    pub fn cluster_to_lba(&self, cluster: u32) -> u64 {
        if cluster < 2 {
            return self.start_lba + self.bpb.data_start_sector;
        }
        let cluster_offset = (cluster - 2) as u64 * self.bpb.sectors_per_cluster as u64;
        self.start_lba + self.bpb.data_start_sector + cluster_offset
    }

    /// Read next cluster value from the File Allocation Table (FAT)
    pub fn get_next_cluster(&self, cluster: u32) -> HalResult<u32> {
        let sector_size = self.bpb.bytes_per_sector as u64;
        let fat_offset = (cluster as u64) * 4;
        let fat_sector = self.start_lba + self.bpb.fat_start_sector + (fat_offset / sector_size);
        let byte_in_sector = (fat_offset % sector_size) as usize;

        let mut sector_buf = vec![0u8; sector_size as usize];
        self.device.read_sectors(fat_sector, 1, &mut sector_buf)?;

        let val = LittleEndian::read_u32(&sector_buf[byte_in_sector..byte_in_sector + 4]) & 0x0FFF_FFFF;
        Ok(val)
    }

    /// Build cluster chain for an active or deleted file
    pub fn build_cluster_chain(&self, first_cluster: u32, file_size: u32) -> HalResult<Vec<DataRun>> {
        if first_cluster < 2 || file_size == 0 {
            return Ok(Vec::new());
        }

        let mut current_cluster = first_cluster;
        let mut runs = Vec::new();
        let mut cluster_list = Vec::new();

        // Step 1: Attempt to read live FAT table chain
        while current_cluster >= 2 && current_cluster < 0x0FFF_FFF8 {
            cluster_list.push(current_cluster);
            match self.get_next_cluster(current_cluster) {
                Ok(next) => {
                    if next == current_cluster || next < 2 || next == 0 {
                        // Pointer was zeroed upon deletion (FAT file deletion behavior)
                        break;
                    }
                    current_cluster = next;
                }
                Err(_) => break,
            }
        }

        // Step 2: If FAT chain was truncated (deleted file), estimate contiguous clusters
        let needed_clusters = ((file_size + self.bpb.cluster_size - 1) / self.bpb.cluster_size) as usize;
        if cluster_list.len() < needed_clusters {
            let missing = needed_clusters - cluster_list.len();
            let mut next_contiguous = cluster_list.last().copied().unwrap_or(first_cluster);
            for _ in 0..missing {
                next_contiguous += 1;
                cluster_list.push(next_contiguous);
            }
        }

        // Step 3: Compress contiguous clusters into DataRuns
        if let Some(&first) = cluster_list.first() {
            let mut run_start = first;
            let mut run_len = 1u64;

            for &c in &cluster_list[1..] {
                if c == run_start + run_len as u32 {
                    run_len += 1;
                } else {
                    runs.push(DataRun {
                        cluster_offset: run_start as u64,
                        cluster_count: run_len,
                    });
                    run_start = c;
                    run_len = 1;
                }
            }
            runs.push(DataRun {
                cluster_offset: run_start as u64,
                cluster_count: run_len,
            });
        }

        Ok(runs)
    }

    /// Traverse directory cluster and discover entries (including deleted 0xE5 items)
    pub fn parse_directory_cluster(
        &self,
        cluster: u32,
    ) -> HalResult<Vec<FatDirectoryEntry>> {
        let cluster_bytes = self.bpb.cluster_size as usize;
        let lba = self.cluster_to_lba(cluster);
        let mut cluster_buf = vec![0u8; cluster_bytes];
        self.device.read_sectors(lba, self.bpb.sectors_per_cluster as u64, &mut cluster_buf)?;

        let mut entries = Vec::new();
        let mut lfn_builder = LfnBuilder::new();

        for chunk in cluster_buf.chunks_exact(32) {
            let first_byte = chunk[0];
            if first_byte == 0x00 {
                // End of directory listing
                break;
            }

            let attr = chunk[11];
            if attr == FAT_ATTR_LFN {
                lfn_builder.add_lfn_entry(chunk);
                continue;
            }

            // Skip volume ID entries
            if (attr & FAT_ATTR_VOLUME_ID) != 0 {
                lfn_builder.reset();
                continue;
            }

            let is_deleted = first_byte == 0xE5;
            let is_dir = (attr & FAT_ATTR_DIRECTORY) != 0;

            let mut short_name_raw = [0u8; 11];
            short_name_raw.copy_from_slice(&chunk[0..11]);
            let chk = LfnBuilder::calculate_checksum(&short_name_raw);
            let long_name = lfn_builder.build_filename(chk);
            lfn_builder.reset();

            let base_name = String::from_utf8_lossy(&short_name_raw[0..8]).trim().to_string();
            let ext = String::from_utf8_lossy(&short_name_raw[8..11]).trim().to_string();
            let short_name = if ext.is_empty() {
                base_name
            } else {
                format!("{}.{}", base_name, ext)
            };

            let cluster_hi = LittleEndian::read_u16(&chunk[20..22]) as u32;
            let cluster_lo = LittleEndian::read_u16(&chunk[26..28]) as u32;
            let first_cluster = (cluster_hi << 16) | cluster_lo;
            let file_size = LittleEndian::read_u32(&chunk[28..32]);

            let time = LittleEndian::read_u16(&chunk[22..24]);
            let date = LittleEndian::read_u16(&chunk[24..26]);
            let modified_at = fat_datetime_to_unix(date, time);

            entries.push(FatDirectoryEntry {
                is_deleted,
                is_directory: is_dir,
                short_name,
                long_name,
                first_cluster,
                file_size,
                created_at: modified_at,
                modified_at,
            });
        }

        Ok(entries)
    }
}

fn fat_datetime_to_unix(date: u16, time: u16) -> i64 {
    let year = ((date >> 9) & 0x7F) as i32 + 1980;
    let month = ((date >> 5) & 0x0F) as u32;
    let day = (date & 0x1F) as u32;

    let hour = ((time >> 11) & 0x1F) as u32;
    let minute = ((time >> 5) & 0x3F) as u32;
    let second = ((time & 0x1F) * 2) as u32;

    chrono::NaiveDate::from_ymd_opt(year, month.max(1), day.max(1))
        .and_then(|d| d.and_hms_opt(hour.min(23), minute.min(59), second.min(59)))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0)
}
