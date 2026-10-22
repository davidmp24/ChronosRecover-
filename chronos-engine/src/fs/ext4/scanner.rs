use crate::fs::ext4::extents::ExtentsParser;
use crate::hal::device::StorageDevice;
use crate::hal::error::{HalError, HalResult};
use byteorder::{ByteOrder, LittleEndian};
use chronos_proto::recovery::{DiscoveredFile, FileCategory, FileStatus};
use std::sync::Arc;
use uuid::Uuid;

pub struct Ext4Superblock {
    pub inodes_count: u32,
    pub blocks_count: u64,
    pub block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_size: u16,
    pub first_data_block: u32,
}

pub struct Ext4Scanner {
    device: Arc<dyn StorageDevice>,
    start_lba: u64,
    sb: Ext4Superblock,
}

impl Ext4Scanner {
    pub fn new(device: Arc<dyn StorageDevice>, start_lba: u64) -> HalResult<Self> {
        let mut sb_buf = vec![0u8; 2048];
        device.read_sectors(start_lba, 4, &mut sb_buf)?;

        let sb_slice = &sb_buf[1024..];
        let magic = LittleEndian::read_u16(&sb_slice[0x38..0x3A]);
        if magic != 0xEF53 {
            return Err(HalError::IoError("Invalid EXT4 Superblock magic".into()));
        }

        let inodes_count = LittleEndian::read_u32(&sb_slice[0x00..0x04]);
        let blocks_count = LittleEndian::read_u32(&sb_slice[0x04..0x08]) as u64;
        let s_log_block_size = LittleEndian::read_u32(&sb_slice[0x18..0x1C]);
        let block_size = 1024u32 << (s_log_block_size.min(6));
        let blocks_per_group = LittleEndian::read_u32(&sb_slice[0x20..0x24]);
        let inodes_per_group = LittleEndian::read_u32(&sb_slice[0x28..0x2C]);
        let inode_size = LittleEndian::read_u16(&sb_slice[0x58..0x5A]);
        let first_data_block = LittleEndian::read_u32(&sb_slice[0x14..0x18]);

        Ok(Self {
            device,
            start_lba,
            sb: Ext4Superblock {
                inodes_count,
                blocks_count,
                block_size,
                blocks_per_group,
                inodes_per_group,
                inode_size: if inode_size == 0 { 256 } else { inode_size },
                first_data_block,
            },
        })
    }

    /// Scan inode table for deleted inodes (where dtime != 0 and links_count == 0)
    pub fn scan_deleted_inodes<F>(
        &self,
        session_id: &str,
        max_inodes: u32,
        mut on_file_found: F,
    ) -> HalResult<u64>
    where
        F: FnMut(DiscoveredFile),
    {
        let mut total_found = 0u64;
        // Group descriptor table starts at block 1 (if 1KB block) or block 0 (if 4KB block)
        let gd_block = if self.sb.block_size == 1024 { 2 } else { 1 };
        let gd_offset = (gd_block as u64) * (self.sb.block_size as u64);

        let mut gd_buf = vec![0u8; 64];
        self.device.read_at(self.start_lba * 512 + gd_offset, &mut gd_buf)?;

        let inode_table_block = LittleEndian::read_u32(&gd_buf[8..12]) as u64;
        let inode_table_offset = self.start_lba * 512 + (inode_table_block * self.sb.block_size as u64);

        let inode_size = self.sb.inode_size as usize;
        let mut inode_buf = vec![0u8; inode_size];

        for ino in 11..max_inodes.min(self.sb.inodes_per_group) {
            let offset = inode_table_offset + ((ino as u64) * inode_size as u64);
            if self.device.read_at(offset, &mut inode_buf).is_ok() {
                let mode = LittleEndian::read_u16(&inode_buf[0..2]);
                let size_lo = LittleEndian::read_u32(&inode_buf[4..8]) as u64;
                let links_count = LittleEndian::read_u16(&inode_buf[26..28]);
                let dtime = LittleEndian::read_u32(&inode_buf[20..24]);

                // A deleted inode has links_count == 0 and dtime != 0
                if links_count == 0 && dtime != 0 && size_lo > 0 {
                    let extents = ExtentsParser::parse_extent_header(&inode_buf[40..100]);
                    let runs = extents.as_ref().map(|e| ExtentsParser::to_proto_runs(e)).unwrap_or_default();
                    let start_lba = extents.as_ref().and_then(|e| e.first()).map(|e| e.physical_block * (self.sb.block_size as u64 / 512)).unwrap_or(0);

                    let file = DiscoveredFile {
                        id: format!("ext4-ino-{}", ino),
                        session_id: session_id.to_string(),
                        partition_index: 1,
                        path: format!("/<ext4_deleted_inode_{}>", ino),
                        filename: format!("recovered_inode_{}", ino),
                        extension: "bin".into(),
                        size_bytes: size_lo,
                        status: FileStatus::FileStatusDeleted as i32,
                        category: FileCategory::FileCategoryUnknown as i32,
                        mime_type: "application/octet-stream".into(),
                        created_at: 0,
                        modified_at: dtime as i64,
                        accessed_at: 0,
                        deleted_at: dtime as i64,
                        recoverability_score: 85,
                        is_directory: (mode & 0x4000) != 0,
                        data_runs: runs,
                        starting_lba: start_lba,
                        md5_hash: String::new(),
                    };

                    on_file_found(file);
                    total_found += 1;
                }
            }
        }

        Ok(total_found)
    }
}
