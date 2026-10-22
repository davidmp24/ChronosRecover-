use crate::hal::device::PartitionDescriptor;
use byteorder::{ByteOrder, LittleEndian};

#[derive(Debug, Clone)]
pub struct MbrPartitionEntry {
    pub status: u8,
    pub start_chs: [u8; 3],
    pub partition_type: u8,
    pub end_chs: [u8; 3],
    pub start_lba: u32,
    pub sector_count: u32,
}

pub struct MbrParser;

impl MbrParser {
    pub const BOOT_SIGNATURE: u16 = 0xAA55;
    pub const MBR_SIZE: usize = 512;

    pub fn parse(sector_0: &[u8]) -> Option<Vec<PartitionDescriptor>> {
        if sector_0.len() < Self::MBR_SIZE {
            return None;
        }

        let sig = LittleEndian::read_u16(&sector_0[510..512]);
        if sig != Self::BOOT_SIGNATURE {
            return None;
        }

        let mut partitions = Vec::new();
        // 4 standard partition entries located at offsets 446, 462, 478, 494
        for i in 0..4 {
            let offset = 446 + i * 16;
            let entry_bytes = &sector_0[offset..offset + 16];
            let part_type = entry_bytes[4];

            // If partition type is 0x00 (empty) or 0xEE (protective MBR for GPT), skip or handle
            if part_type == 0x00 || part_type == 0xEE {
                continue;
            }

            let start_lba = LittleEndian::read_u32(&entry_bytes[8..12]) as u64;
            let sector_count = LittleEndian::read_u32(&entry_bytes[12..16]) as u64;

            if sector_count == 0 {
                continue;
            }

            let fs_type = match part_type {
                0x07 => "NTFS/exFAT",
                0x0B | 0x0C => "FAT32",
                0x04 | 0x06 | 0x0E => "FAT16",
                0x83 => "Linux EXT",
                0x82 => "Linux Swap",
                _ => "Unknown MBR",
            };

            partitions.push(PartitionDescriptor {
                index: i as u32 + 1,
                start_lba,
                sector_count,
                size_bytes: sector_count * 512,
                fs_type: fs_type.to_string(),
                label: format!("MBR Partition {}", i + 1),
                mount_point: None,
                volume_guid: None,
                is_orphan: false,
                cluster_size: 4096,
            });
        }

        Some(partitions)
    }

    pub fn is_protective_mbr(sector_0: &[u8]) -> bool {
        if sector_0.len() < Self::MBR_SIZE {
            return false;
        }
        let sig = LittleEndian::read_u16(&sector_0[510..512]);
        if sig != Self::BOOT_SIGNATURE {
            return false;
        }
        // First entry partition type == 0xEE
        sector_0[446 + 4] == 0xEE
    }
}
