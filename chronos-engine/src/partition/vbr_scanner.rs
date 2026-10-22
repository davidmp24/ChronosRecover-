use crate::hal::device::{PartitionDescriptor, StorageDevice};
use crate::hal::error::HalResult;
use byteorder::{ByteOrder, LittleEndian};
use std::sync::Arc;
use tracing::{debug, info};

pub const NTFS_VBR_SIGNATURE: &[u8; 8] = b"\xEB\x52\x90NTFS ";
pub const FAT32_VBR_SIGNATURE: &[u8; 8] = b"\xEB\x58\x90MSDOS";
pub const EXFAT_VBR_SIGNATURE: &[u8; 8] = b"\xEB\x76\x90EXFAT ";
pub const EXT4_SUPERBLOCK_MAGIC: u16 = 0xEF53;

#[derive(Debug, Clone)]
pub struct OrphanPartition {
    pub start_lba: u64,
    pub fs_type: String,
    pub sector_count: u64,
    pub cluster_size: u32,
    pub label: String,
}

pub struct VbrScanner;

impl VbrScanner {
    /// Inspect a single sector (512 bytes) to detect whether it is an orphan VBR
    pub fn inspect_sector(sector_data: &[u8], lba: u64) -> Option<OrphanPartition> {
        if sector_data.len() < 512 {
            return None;
        }

        // Must end with 0x55 0xAA boot signature
        if sector_data[510] != 0x55 || sector_data[511] != 0xAA {
            return None;
        }

        // 1. NTFS VBR
        if sector_data.starts_with(NTFS_VBR_SIGNATURE) {
            let bytes_per_sector = LittleEndian::read_u16(&sector_data[11..13]) as u32;
            let sectors_per_cluster = sector_data[13] as u32;
            let total_sectors = LittleEndian::read_u64(&sector_data[40..48]);
            let cluster_size = bytes_per_sector.saturating_mul(sectors_per_cluster).max(512);

            return Some(OrphanPartition {
                start_lba: lba,
                fs_type: "NTFS".to_string(),
                sector_count: total_sectors,
                cluster_size,
                label: format!("Orphan NTFS (LBA 0x{:08X})", lba),
            });
        }

        // 2. FAT32 VBR
        if sector_data.starts_with(FAT32_VBR_SIGNATURE) {
            let bytes_per_sector = LittleEndian::read_u16(&sector_data[11..13]) as u32;
            let sectors_per_cluster = sector_data[13] as u32;
            let total_sectors_16 = LittleEndian::read_u16(&sector_data[19..21]) as u64;
            let total_sectors_32 = LittleEndian::read_u32(&sector_data[32..36]) as u64;
            let total_sectors = if total_sectors_16 != 0 { total_sectors_16 } else { total_sectors_32 };
            let cluster_size = bytes_per_sector.saturating_mul(sectors_per_cluster).max(512);

            let vol_label = String::from_utf8_lossy(&sector_data[71..82]).trim().to_string();
            let label = if vol_label.is_empty() || vol_label == "NO NAME" {
                format!("Orphan FAT32 (LBA 0x{:08X})", lba)
            } else {
                vol_label
            };

            return Some(OrphanPartition {
                start_lba: lba,
                fs_type: "FAT32".to_string(),
                sector_count: total_sectors,
                cluster_size,
                label,
            });
        }

        // 3. exFAT VBR
        if sector_data.starts_with(EXFAT_VBR_SIGNATURE) {
            let bytes_per_sector_shift = sector_data[108];
            let sectors_per_cluster_shift = sector_data[109];
            let bytes_per_sector = 1u32 << (bytes_per_sector_shift.min(16));
            let cluster_size = bytes_per_sector << (sectors_per_cluster_shift.min(16));
            let volume_length_sectors = LittleEndian::read_u64(&sector_data[72..80]);

            return Some(OrphanPartition {
                start_lba: lba,
                fs_type: "exFAT".to_string(),
                sector_count: volume_length_sectors,
                cluster_size,
                label: format!("Orphan exFAT (LBA 0x{:08X})", lba),
            });
        }

        None
    }

    /// Check for Linux EXT4 Superblock (offset 1024 from start of partition)
    pub fn inspect_ext4_superblock(block_data: &[u8], lba: u64) -> Option<OrphanPartition> {
        if block_data.len() < 1024 + 1024 {
            return None;
        }

        let sb = &block_data[1024..];
        let magic = LittleEndian::read_u16(&sb[0x38..0x3A]);
        if magic != EXT4_SUPERBLOCK_MAGIC {
            return None;
        }

        let s_blocks_count_lo = LittleEndian::read_u32(&sb[0x04..0x08]) as u64;
        let s_log_block_size = LittleEndian::read_u32(&sb[0x18..0x1C]);
        let block_size = 1024u32 << s_log_block_size.min(6);
        let volume_name = String::from_utf8_lossy(&sb[0x78..0x88]).trim().trim_matches('\0').to_string();

        let label = if volume_name.is_empty() {
            format!("Orphan EXT4 (LBA 0x{:08X})", lba)
        } else {
            volume_name
        };

        Some(OrphanPartition {
            start_lba: lba,
            fs_type: "EXT4".to_string(),
            sector_count: s_blocks_count_lo * (block_size as u64 / 512),
            cluster_size: block_size,
            label,
        })
    }

    /// Scan a device to discover active or orphaned partitions
    pub fn scan_partitions(device: &Arc<dyn StorageDevice>) -> HalResult<Vec<PartitionDescriptor>> {
        let mut partitions = Vec::new();
        let mut sector_0 = vec![0u8; 512];
        if device.read_sectors(0, 1, &mut sector_0).is_ok() {
            // Check MBR / Protective MBR
            if mbr::MbrParser::is_protective_mbr(&sector_0) {
                info!("Protective MBR detected, parsing Primary GPT at LBA 1..33");
                let mut gpt_header_sector = vec![0u8; 512];
                if device.read_sectors(1, 1, &mut gpt_header_sector).is_ok() {
                    if let Some(gpt_header) = gpt::GptParser::parse_header(&gpt_header_sector) {
                        let total_entry_bytes = (gpt_header.num_partition_entries * gpt_header.size_of_partition_entry) as usize;
                        let sectors_to_read = (total_entry_bytes as u64 + 511) / 512;
                        let mut entries_buf = vec![0u8; (sectors_to_read * 512) as usize];
                        if device.read_sectors(gpt_header.partition_entry_lba, sectors_to_read, &mut entries_buf).is_ok() {
                            if let Some(gpt_parts) = gpt::GptParser::parse_entries(
                                &entries_buf,
                                gpt_header.num_partition_entries,
                                gpt_header.size_of_partition_entry,
                                gpt_header.partition_entry_array_crc32,
                            ) {
                                partitions.extend(gpt_parts);
                            }
                        }
                    }
                }
            } else if let Some(mbr_parts) = mbr::MbrParser::parse(&sector_0) {
                if !mbr_parts.is_empty() {
                    partitions.extend(mbr_parts);
                }
            }
        }

        // If no partition tables were found or if disk was wiped, scan common partition alignments
        if partitions.is_empty() {
            info!("No valid partition table found. Scanning for Orphan VBRs...");
            let common_lbas = [63, 128, 2048, 4096, 8192, 16384, 32768, 65536];
            let mut sec_buf = vec![0u8; 2048];

            for &test_lba in &common_lbas {
                if test_lba >= device.descriptor().geometry.total_sectors {
                    continue;
                }
                if device.read_sectors(test_lba, 4, &mut sec_buf).is_ok() {
                    if let Some(orphan) = Self::inspect_sector(&sec_buf[0..512], test_lba) {
                        partitions.push(PartitionDescriptor {
                            index: partitions.len() as u32 + 1,
                            start_lba: orphan.start_lba,
                            sector_count: orphan.sector_count,
                            size_bytes: orphan.sector_count * 512,
                            fs_type: orphan.fs_type,
                            label: orphan.label,
                            mount_point: None,
                            volume_guid: None,
                            is_orphan: true,
                            cluster_size: orphan.cluster_size,
                        });
                    } else if let Some(ext4_orphan) = Self::inspect_ext4_superblock(&sec_buf, test_lba) {
                        partitions.push(PartitionDescriptor {
                            index: partitions.len() as u32 + 1,
                            start_lba: ext4_orphan.start_lba,
                            sector_count: ext4_orphan.sector_count,
                            size_bytes: ext4_orphan.sector_count * 512,
                            fs_type: ext4_orphan.fs_type,
                            label: ext4_orphan.label,
                            mount_point: None,
                            volume_guid: None,
                            is_orphan: true,
                            cluster_size: ext4_orphan.cluster_size,
                        });
                    }
                }
            }
        }

        Ok(partitions)
    }
}
