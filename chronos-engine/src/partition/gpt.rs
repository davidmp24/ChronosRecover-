use crate::hal::device::PartitionDescriptor;
use byteorder::{ByteOrder, LittleEndian};
use crc32fast::Hasher;
use tracing::warn;

pub const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";

#[derive(Debug, Clone)]
pub struct GptHeader {
    pub revision: u32,
    pub header_size: u32,
    pub header_crc32: u32,
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_entry_lba: u64,
    pub num_partition_entries: u32,
    pub size_of_partition_entry: u32,
    pub partition_entry_array_crc32: u32,
}

pub struct GptParser;

impl GptParser {
    pub fn parse_header(sector_1: &[u8]) -> Option<GptHeader> {
        if sector_1.len() < 92 {
            return None;
        }

        if &sector_1[0..8] != GPT_SIGNATURE {
            return None;
        }

        let header_size = LittleEndian::read_u32(&sector_1[12..16]);
        if (header_size as usize) < 92 || (header_size as usize) > sector_1.len() {
            return None;
        }

        let stored_crc32 = LittleEndian::read_u32(&sector_1[16..20]);

        // Validate Header CRC32 with CRC field zeroed
        let mut header_copy = sector_1[0..header_size as usize].to_vec();
        header_copy[16..20].fill(0);
        let mut hasher = Hasher::new();
        hasher.update(&header_copy);
        let calculated_crc = hasher.finalize();

        if calculated_crc != stored_crc32 {
            warn!(
                calculated = calculated_crc,
                stored = stored_crc32,
                "GPT Header CRC32 checksum mismatch"
            );
            return None;
        }

        let mut disk_guid = [0u8; 16];
        disk_guid.copy_from_slice(&sector_1[56..72]);

        Some(GptHeader {
            revision: LittleEndian::read_u32(&sector_1[8..12]),
            header_size,
            header_crc32: stored_crc32,
            current_lba: LittleEndian::read_u64(&sector_1[24..32]),
            backup_lba: LittleEndian::read_u64(&sector_1[32..40]),
            first_usable_lba: LittleEndian::read_u64(&sector_1[40..48]),
            last_usable_lba: LittleEndian::read_u64(&sector_1[48..56]),
            disk_guid,
            partition_entry_lba: LittleEndian::read_u64(&sector_1[72..80]),
            num_partition_entries: LittleEndian::read_u32(&sector_1[80..84]),
            size_of_partition_entry: LittleEndian::read_u32(&sector_1[84..88]),
            partition_entry_array_crc32: LittleEndian::read_u32(&sector_1[88..92]),
        })
    }

    pub fn parse_entries(
        entries_data: &[u8],
        num_entries: u32,
        entry_size: u32,
        expected_crc: u32,
    ) -> Option<Vec<PartitionDescriptor>> {
        let total_bytes = (num_entries as usize) * (entry_size as usize);
        if entries_data.len() < total_bytes {
            return None;
        }

        // Validate Partition Entry Array CRC32
        let mut hasher = Hasher::new();
        hasher.update(&entries_data[..total_bytes]);
        if hasher.finalize() != expected_crc {
            warn!("GPT Partition entries CRC32 validation failed");
            // Still attempt best-effort recovery of valid entries
        }

        let mut partitions = Vec::new();

        for i in 0..num_entries as usize {
            let offset = i * (entry_size as usize);
            let entry = &entries_data[offset..offset + (entry_size as usize)];

            // Check if Type GUID is all zeros (unused entry)
            if entry[0..16].iter().all(|&b| b == 0) {
                continue;
            }

            let start_lba = LittleEndian::read_u64(&entry[32..40]);
            let end_lba = LittleEndian::read_u64(&entry[40..48]);
            if end_lba < start_lba {
                continue;
            }
            let sector_count = end_lba - start_lba + 1;

            // Partition Name (UTF-16LE, 72 bytes at offset 56)
            let mut name_u16 = Vec::new();
            for chunk in entry[56..56 + 72].chunks_exact(2) {
                let code = LittleEndian::read_u16(chunk);
                if code == 0 {
                    break;
                }
                name_u16.push(code);
            }
            let partition_name = String::from_utf16_lossy(&name_u16).trim().to_string();

            // Detect Partition Type GUID
            let type_guid = &entry[0..16];
            let fs_type = match type_guid {
                // Microsoft Basic Data: EBD0A0A2-B9E5-4433-87C0-68B6B72699C7
                [0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7] => {
                    "NTFS/FAT32/exFAT"
                }
                // Linux Filesystem Data: 0FC63DAF-8483-4772-8E79-3D69D8477DE4
                [0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4] => {
                    "EXT4/Linux"
                }
                // EFI System Partition: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
                [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B] => {
                    "FAT32 (EFI)"
                }
                _ => "GPT Data",
            };

            let label = if partition_name.is_empty() {
                format!("GPT Partition {}", i + 1)
            } else {
                partition_name
            };

            partitions.push(PartitionDescriptor {
                index: i as u32 + 1,
                start_lba,
                sector_count,
                size_bytes: sector_count * 512,
                fs_type: fs_type.to_string(),
                label,
                mount_point: None,
                volume_guid: None,
                is_orphan: false,
                cluster_size: 4096,
            });
        }

        Some(partitions)
    }
}
