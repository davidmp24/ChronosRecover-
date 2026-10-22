use crate::fs::ntfs::attributes::filetime_to_unix;
use byteorder::{ByteOrder, LittleEndian};
use chronos_proto::recovery::{HistoryEvent, HistoryEventType};
use uuid::Uuid;

pub const USN_REASON_DATA_OVERWRITE: u32 = 0x00000001;
pub const USN_REASON_DATA_EXTEND: u32 = 0x00000002;
pub const USN_REASON_DATA_TRUNCATION: u32 = 0x00000004;
pub const USN_REASON_FILE_CREATE: u32 = 0x00000100;
pub const USN_REASON_FILE_DELETE: u32 = 0x00000200;
pub const USN_REASON_SECURITY_CHANGE: u32 = 0x00000800;
pub const USN_REASON_RENAME_OLD_NAME: u32 = 0x00001000;
pub const USN_REASON_RENAME_NEW_NAME: u32 = 0x00002000;

#[derive(Debug, Clone)]
pub struct UsnRecordV2 {
    pub usn: u64,
    pub timestamp: i64,
    pub file_reference_number: u64,
    pub parent_file_reference_number: u64,
    pub reason: u32,
    pub filename: String,
}

pub struct UsnJournalParser;

impl UsnJournalParser {
    /// Parse a single USN_RECORD (v2 or v3) from raw buffer slice
    pub fn parse_record(slice: &[u8]) -> Option<(UsnRecordV2, usize)> {
        if slice.len() < 60 {
            return None;
        }

        let record_length = LittleEndian::read_u32(&slice[0..4]) as usize;
        if record_length < 60 || record_length > slice.len() {
            return None;
        }

        let major_version = LittleEndian::read_u16(&slice[4..6]);
        if major_version != 2 && major_version != 3 {
            return None;
        }

        let file_ref = LittleEndian::read_u64(&slice[8..16]) & 0x0000_FFFF_FFFF_FFFF;
        let parent_ref = LittleEndian::read_u64(&slice[16..24]) & 0x0000_FFFF_FFFF_FFFF;
        let usn = LittleEndian::read_u64(&slice[24..32]);
        let filetime = LittleEndian::read_u64(&slice[32..40]);
        let reason = LittleEndian::read_u32(&slice[40..44]);

        let fn_len = LittleEndian::read_u16(&slice[56..58]) as usize;
        let fn_offset = LittleEndian::read_u16(&slice[58..60]) as usize;

        let filename = if fn_offset + fn_len <= record_length {
            let num_chars = fn_len / 2;
            let mut u16_chars = Vec::with_capacity(num_chars);
            for i in 0..num_chars {
                let off = fn_offset + i * 2;
                u16_chars.push(LittleEndian::read_u16(&slice[off..off + 2]));
            }
            String::from_utf16_lossy(&u16_chars)
        } else {
            String::new()
        };

        let timestamp = filetime_to_unix(filetime);

        Some((
            UsnRecordV2 {
                usn,
                timestamp,
                file_reference_number: file_ref,
                parent_file_reference_number: parent_ref,
                reason,
                filename,
            },
            record_length,
        ))
    }

    /// Stream parse a chunk of $UsnJrnl:$J and convert records to gRPC HistoryEvents
    pub fn parse_journal_chunk(
        chunk_data: &[u8],
        session_id: &str,
    ) -> Vec<HistoryEvent> {
        let mut events = Vec::new();
        let mut cursor = 0;

        while cursor + 60 <= chunk_data.len() {
            // Find non-zero record start (USN journal has sparse zero gaps)
            if chunk_data[cursor..cursor + 4].iter().all(|&b| b == 0) {
                cursor += 8; // Align forward
                continue;
            }

            match Self::parse_record(&chunk_data[cursor..]) {
                Some((rec, rec_len)) => {
                    cursor += rec_len;

                    let (event_type, reason_str) = Self::map_reason(rec.reason);

                    events.push(HistoryEvent {
                        id: Uuid::new_v4().to_string(),
                        session_id: session_id.to_string(),
                        timestamp: rec.timestamp,
                        event_type: event_type as i32,
                        source_path: rec.filename.clone(),
                        destination_path: String::new(),
                        usn: rec.usn,
                        file_reference_number: rec.file_reference_number,
                        reason_flags: reason_str,
                        details: format!(
                            "Parent MFT #{}, Flags: 0x{:08X}",
                            rec.parent_file_reference_number, rec.reason
                        ),
                    });
                }
                None => {
                    cursor += 8;
                }
            }
        }

        events
    }

    fn map_reason(reason: u32) -> (HistoryEventType, String) {
        if (reason & USN_REASON_FILE_DELETE) != 0 {
            (HistoryEventType::HistoryEventDeleted, "FILE_DELETE".into())
        } else if (reason & USN_REASON_FILE_CREATE) != 0 {
            (HistoryEventType::HistoryEventCreated, "FILE_CREATE".into())
        } else if (reason & (USN_REASON_RENAME_OLD_NAME | USN_REASON_RENAME_NEW_NAME)) != 0 {
            (HistoryEventType::HistoryEventRenamed, "FILE_RENAME".into())
        } else if (reason & USN_REASON_DATA_TRUNCATION) != 0 {
            (HistoryEventType::HistoryEventDataTruncate, "DATA_TRUNCATE".into())
        } else if (reason & USN_REASON_DATA_EXTEND) != 0 {
            (HistoryEventType::HistoryEventDataExtend, "DATA_EXTEND".into())
        } else if (reason & USN_REASON_SECURITY_CHANGE) != 0 {
            (HistoryEventType::HistoryEventSecurityChange, "SECURITY_CHANGE".into())
        } else {
            (HistoryEventType::HistoryEventUnknown, format!("REASON_0x{:08X}", reason))
        }
    }
}
