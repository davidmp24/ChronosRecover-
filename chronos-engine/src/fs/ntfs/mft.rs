use crate::fs::ntfs::attributes::{
    AttributeParser, DataAttribute, FileNameAttribute, StandardInformation,
    ATTR_DATA, ATTR_FILE_NAME, ATTR_STANDARD_INFORMATION,
};
use byteorder::{ByteOrder, LittleEndian};

pub const MFT_RECORD_MAGIC_FILE: u32 = 0x454C4946; // "FILE"
pub const MFT_RECORD_FLAG_IN_USE: u16 = 0x0001;
pub const MFT_RECORD_FLAG_DIRECTORY: u16 = 0x0002;

#[derive(Debug, Clone)]
pub struct MftRecord {
    pub record_number: u64,
    pub is_in_use: bool,
    pub is_directory: bool,
    pub sequence_number: u16,
    pub base_record_reference: u64,
    pub standard_info: Option<StandardInformation>,
    pub file_names: Vec<FileNameAttribute>,
    pub data_attributes: Vec<DataAttribute>,
}

pub struct MftParser;

impl MftParser {
    pub fn parse_record(raw_record: &[u8], record_number: u64) -> Option<MftRecord> {
        if raw_record.len() < 1024 {
            return None;
        }

        let magic = LittleEndian::read_u32(&raw_record[0..4]);
        if magic != MFT_RECORD_MAGIC_FILE {
            return None;
        }

        // Apply Update Sequence Array (USA) fixups
        let mut fixed_record = raw_record[0..1024].to_vec();
        if !Self::apply_fixups(&mut fixed_record) {
            return None;
        }

        let flags = LittleEndian::read_u16(&fixed_record[22..24]);
        let is_in_use = (flags & MFT_RECORD_FLAG_IN_USE) != 0;
        let is_directory = (flags & MFT_RECORD_FLAG_DIRECTORY) != 0;
        let sequence_number = LittleEndian::read_u16(&fixed_record[16..18]);
        let base_ref = LittleEndian::read_u64(&fixed_record[32..40]) & 0x0000_FFFF_FFFF_FFFF;

        let mut attr_offset = LittleEndian::read_u16(&fixed_record[20..22]) as usize;

        let mut standard_info = None;
        let mut file_names = Vec::new();
        let mut data_attributes = Vec::new();

        // Walk attributes linked list
        while attr_offset + 8 <= fixed_record.len() {
            let attr_type = LittleEndian::read_u32(&fixed_record[attr_offset..attr_offset + 4]);
            if attr_type == 0xFFFF_FFFF || attr_type == 0 {
                // End of attributes marker
                break;
            }

            let attr_len = LittleEndian::read_u32(&fixed_record[attr_offset + 4..attr_offset + 8]) as usize;
            if attr_len == 0 || attr_offset + attr_len > fixed_record.len() {
                break;
            }

            let attr_slice = &fixed_record[attr_offset..attr_offset + attr_len];

            match attr_type {
                ATTR_STANDARD_INFORMATION => {
                    let non_resident = attr_slice[8] != 0;
                    if !non_resident && attr_slice.len() >= 24 {
                        let content_len = LittleEndian::read_u32(&attr_slice[16..20]) as usize;
                        let content_offset = LittleEndian::read_u16(&attr_slice[20..22]) as usize;
                        if content_offset + content_len <= attr_slice.len() {
                            standard_info = AttributeParser::parse_standard_information(
                                &attr_slice[content_offset..content_offset + content_len],
                            );
                        }
                    }
                }
                ATTR_FILE_NAME => {
                    let non_resident = attr_slice[8] != 0;
                    if !non_resident && attr_slice.len() >= 24 {
                        let content_len = LittleEndian::read_u32(&attr_slice[16..20]) as usize;
                        let content_offset = LittleEndian::read_u16(&attr_slice[20..22]) as usize;
                        if content_offset + content_len <= attr_slice.len() {
                            if let Some(fn_attr) = AttributeParser::parse_file_name(
                                &attr_slice[content_offset..content_offset + content_len],
                            ) {
                                file_names.push(fn_attr);
                            }
                        }
                    }
                }
                ATTR_DATA => {
                    if let Some(data_attr) = AttributeParser::parse_data(attr_slice) {
                        data_attributes.push(data_attr);
                    }
                }
                _ => {}
            }

            attr_offset += attr_len;
        }

        Some(MftRecord {
            record_number,
            is_in_use,
            is_directory,
            sequence_number,
            base_record_reference: base_ref,
            standard_info,
            file_names,
            data_attributes,
        })
    }

    /// Apply Update Sequence Array (USA) fixups across 512-byte sectors
    fn apply_fixups(record: &mut [u8]) -> bool {
        let fixup_offset = LittleEndian::read_u16(&record[4..6]) as usize;
        let fixup_count = LittleEndian::read_u16(&record[6..8]) as usize;

        if fixup_count == 0 || fixup_offset + fixup_count * 2 > record.len() {
            return false;
        }

        let expected_signature = LittleEndian::read_u16(&record[fixup_offset..fixup_offset + 2]);
        let num_sectors = fixup_count - 1;

        for i in 0..num_sectors {
            let sector_end_offset = (i + 1) * 512 - 2;
            if sector_end_offset + 2 > record.len() {
                break;
            }
            let written_signature = LittleEndian::read_u16(&record[sector_end_offset..sector_end_offset + 2]);
            if written_signature != expected_signature {
                // Fixup check failure - corrupted sector
                return false;
            }

            let replacement_offset = fixup_offset + (i + 1) * 2;
            record[sector_end_offset] = record[replacement_offset];
            record[sector_end_offset + 1] = record[replacement_offset + 1];
        }

        true
    }
}
