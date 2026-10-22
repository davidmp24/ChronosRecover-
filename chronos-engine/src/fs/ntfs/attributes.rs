use crate::fs::ntfs::runlist::{DecodedRun, RunlistDecoder};
use byteorder::{ByteOrder, LittleEndian};

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_OBJECT_ID: u32 = 0x40;
pub const ATTR_DATA: u32 = 0x80;
pub const ATTR_BITMAP: u32 = 0xB0;

// Convert Windows FILETIME (100ns intervals since 1601-01-01) to Unix epoch seconds
pub fn filetime_to_unix(filetime: u64) -> i64 {
    if filetime < 116444736000000000 {
        return 0;
    }
    ((filetime - 116444736000000000) / 10_000_000) as i64
}

#[derive(Debug, Clone)]
pub struct StandardInformation {
    pub created_at: i64,
    pub modified_at: i64,
    pub mft_modified_at: i64,
    pub accessed_at: i64,
    pub file_attributes: u32,
}

#[derive(Debug, Clone)]
pub struct FileNameAttribute {
    pub parent_mft_reference: u64,
    pub allocated_size: u64,
    pub real_size: u64,
    pub flags: u32,
    pub namespace: u8,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub enum DataAttributeContent {
    Resident(Vec<u8>),
    NonResident {
        allocated_size: u64,
        real_size: u64,
        data_runs: Vec<DecodedRun>,
    },
}

#[derive(Debug, Clone)]
pub struct DataAttribute {
    pub name: String,
    pub content: DataAttributeContent,
}

pub struct AttributeParser;

impl AttributeParser {
    pub fn parse_standard_information(data: &[u8]) -> Option<StandardInformation> {
        if data.len() < 48 {
            return None;
        }

        let created = LittleEndian::read_u64(&data[0..8]);
        let modified = LittleEndian::read_u64(&data[8..16]);
        let mft_mod = LittleEndian::read_u64(&data[16..24]);
        let accessed = LittleEndian::read_u64(&data[24..32]);
        let flags = LittleEndian::read_u32(&data[32..36]);

        Some(StandardInformation {
            created_at: filetime_to_unix(created),
            modified_at: filetime_to_unix(modified),
            mft_modified_at: filetime_to_unix(mft_mod),
            accessed_at: filetime_to_unix(accessed),
            file_attributes: flags,
        })
    }

    pub fn parse_file_name(data: &[u8]) -> Option<FileNameAttribute> {
        if data.len() < 66 {
            return None;
        }

        let parent_ref = LittleEndian::read_u64(&data[0..8]) & 0x0000_FFFF_FFFF_FFFF;
        let allocated_size = LittleEndian::read_u64(&data[40..48]);
        let real_size = LittleEndian::read_u64(&data[48..56]);
        let flags = LittleEndian::read_u32(&data[56..60]);
        let filename_len = data[64] as usize;
        let namespace = data[65];

        if data.len() < 66 + filename_len * 2 {
            return None;
        }

        let mut name_u16 = Vec::with_capacity(filename_len);
        for i in 0..filename_len {
            let offset = 66 + i * 2;
            name_u16.push(LittleEndian::read_u16(&data[offset..offset + 2]));
        }
        let filename = String::from_utf16_lossy(&name_u16);

        Some(FileNameAttribute {
            parent_mft_reference: parent_ref,
            allocated_size,
            real_size,
            flags,
            namespace,
            filename,
        })
    }

    pub fn parse_data(attr_slice: &[u8]) -> Option<DataAttribute> {
        if attr_slice.len() < 16 {
            return None;
        }

        let non_resident = attr_slice[8] != 0;
        let name_len = attr_slice[9] as usize;
        let name_offset = LittleEndian::read_u16(&attr_slice[10..12]) as usize;

        let name = if name_len > 0 && name_offset + name_len * 2 <= attr_slice.len() {
            let mut name_u16 = Vec::with_capacity(name_len);
            for i in 0..name_len {
                let off = name_offset + i * 2;
                name_u16.push(LittleEndian::read_u16(&attr_slice[off..off + 2]));
            }
            String::from_utf16_lossy(&name_u16)
        } else {
            String::new()
        };

        if !non_resident {
            // Resident Data
            let content_len = LittleEndian::read_u32(&attr_slice[16..20]) as usize;
            let content_offset = LittleEndian::read_u16(&attr_slice[20..22]) as usize;
            if content_offset + content_len <= attr_slice.len() {
                let content_bytes = attr_slice[content_offset..content_offset + content_len].to_vec();
                Some(DataAttribute {
                    name,
                    content: DataAttributeContent::Resident(content_bytes),
                })
            } else {
                None
            }
        } else {
            // Non-Resident Data
            if attr_slice.len() < 56 {
                return None;
            }
            let runlist_offset = LittleEndian::read_u16(&attr_slice[32..34]) as usize;
            let allocated_size = LittleEndian::read_u64(&attr_slice[40..48]);
            let real_size = LittleEndian::read_u64(&attr_slice[48..56]);

            let runs = if runlist_offset < attr_slice.len() {
                RunlistDecoder::decode(&attr_slice[runlist_offset..])
            } else {
                Vec::new()
            };

            Some(DataAttribute {
                name,
                content: DataAttributeContent::NonResident {
                    allocated_size,
                    real_size,
                    data_runs: runs,
                },
            })
        }
    }
}
