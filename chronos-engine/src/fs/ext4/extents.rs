use byteorder::{ByteOrder, LittleEndian};
use chronos_proto::recovery::DataRun;

pub const EXT4_EXT_MAGIC: u16 = 0xF30A;

#[derive(Debug, Clone)]
pub struct Ext4Extent {
    pub logical_block: u32,
    pub length: u16,
    pub physical_block: u64,
}

pub struct ExtentsParser;

impl ExtentsParser {
    /// Parse extents tree from inode data block (60 bytes at offset 40)
    pub fn parse_extent_header(data: &[u8]) -> Option<Vec<Ext4Extent>> {
        if data.len() < 12 {
            return None;
        }

        let magic = LittleEndian::read_u16(&data[0..2]);
        if magic != EXT4_EXT_MAGIC {
            return None;
        }

        let entries = LittleEndian::read_u16(&data[2..4]) as usize;
        let depth = LittleEndian::read_u16(&data[6..8]);

        // If depth == 0, the header is immediately followed by leaf extents (12 bytes each)
        if depth == 0 {
            let mut extents = Vec::with_capacity(entries);
            for i in 0..entries {
                let off = 12 + i * 12;
                if off + 12 > data.len() {
                    break;
                }
                let ee_block = LittleEndian::read_u32(&data[off..off + 4]);
                let ee_len = LittleEndian::read_u16(&data[off + 4..off + 6]);
                let ee_start_hi = LittleEndian::read_u16(&data[off + 6..off + 8]) as u64;
                let ee_start_lo = LittleEndian::read_u32(&data[off + 8..off + 12]) as u64;
                let physical_block = (ee_start_hi << 32) | ee_start_lo;

                extents.push(Ext4Extent {
                    logical_block: ee_block,
                    length: ee_len,
                    physical_block,
                });
            }
            return Some(extents);
        }

        None
    }

    pub fn to_proto_runs(extents: &[Ext4Extent]) -> Vec<DataRun> {
        extents
            .iter()
            .map(|e| DataRun {
                cluster_offset: e.physical_block,
                cluster_count: e.length as u64,
            })
            .collect()
    }
}
