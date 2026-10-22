use byteorder::{ByteOrder, LittleEndian};

#[derive(Default)]
pub struct LfnBuilder {
    fragments: Vec<(u8, Vec<u16>)>,
    expected_checksum: Option<u8>,
}

impl LfnBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.fragments.clear();
        self.expected_checksum = None;
    }

    /// Process a 32-byte LFN directory entry
    pub fn add_lfn_entry(&mut self, entry: &[u8]) {
        if entry.len() < 32 || entry[11] != 0x0F {
            return;
        }

        let seq = entry[0] & 0x1F;
        let checksum = entry[13];

        if let Some(expected) = self.expected_checksum {
            if expected != checksum {
                self.reset();
            }
        }
        self.expected_checksum = Some(checksum);

        let mut u16_chars = Vec::with_capacity(13);

        // 5 chars from offset 1..11
        for i in 0..5 {
            let off = 1 + i * 2;
            let ch = LittleEndian::read_u16(&entry[off..off + 2]);
            if ch != 0xFFFF && ch != 0x0000 {
                u16_chars.push(ch);
            }
        }

        // 6 chars from offset 14..26
        for i in 0..6 {
            let off = 14 + i * 2;
            let ch = LittleEndian::read_u16(&entry[off..off + 2]);
            if ch != 0xFFFF && ch != 0x0000 {
                u16_chars.push(ch);
            }
        }

        // 2 chars from offset 28..32
        for i in 0..2 {
            let off = 28 + i * 2;
            let ch = LittleEndian::read_u16(&entry[off..off + 2]);
            if ch != 0xFFFF && ch != 0x0000 {
                u16_chars.push(ch);
            }
        }

        self.fragments.push((seq, u16_chars));
    }

    /// Finalize and build complete filename if checksum matches 8.3 short name
    pub fn build_filename(&self, short_name_checksum: u8) -> Option<String> {
        if self.fragments.is_empty() {
            return None;
        }

        if let Some(chk) = self.expected_checksum {
            if chk != short_name_checksum {
                return None;
            }
        }

        // Sort fragments by sequence ascending (1..N)
        let mut sorted = self.fragments.clone();
        sorted.sort_by_key(|&(seq, _)| seq);

        let mut all_chars = Vec::new();
        for (_, chars) in sorted {
            all_chars.extend(chars);
        }

        let name = String::from_utf16_lossy(&all_chars).trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    }

    /// Calculate 8.3 filename checksum
    pub fn calculate_checksum(short_name: &[u8; 11]) -> u8 {
        let mut sum: u8 = 0;
        for &b in short_name {
            sum = ((sum >> 1) | (sum << 7)).wrapping_add(b);
        }
        sum
    }
}
