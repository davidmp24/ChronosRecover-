pub struct JpegFragmentAnalyzer;

impl JpegFragmentAnalyzer {
    pub const RST0: u8 = 0xD0;
    pub const RST7: u8 = 0xD7;

    /// Calculate Shannon Entropy over a 512-byte block (returns 0.0 to 8.0 bits per byte)
    pub fn calculate_entropy(block: &[u8]) -> f64 {
        if block.is_empty() {
            return 0.0;
        }

        let mut counts = [0u32; 256];
        for &b in block {
            counts[b as usize] += 1;
        }

        let total = block.len() as f64;
        let mut entropy = 0.0;

        for &count in &counts {
            if count > 0 {
                let p = (count as f64) / total;
                entropy -= p * p.log2();
            }
        }

        entropy
    }

    /// Scan JPEG data for restart marker continuity (RST0 -> RST1 -> ... -> RST7 -> RST0)
    pub fn verify_restart_marker_continuity(jpeg_stream: &[u8]) -> Option<usize> {
        let mut expected_marker = 0u8;
        let mut first_marker_seen = false;

        let mut i = 0;
        while i + 1 < jpeg_stream.len() {
            if jpeg_stream[i] == 0xFF {
                let marker = jpeg_stream[i + 1];
                if marker >= Self::RST0 && marker <= Self::RST7 {
                    let rst_idx = marker - Self::RST0;
                    if !first_marker_seen {
                        first_marker_seen = true;
                        expected_marker = (rst_idx + 1) % 8;
                    } else if rst_idx != expected_marker {
                        // Broken restart marker sequence detected!
                        // This byte offset represents the fragmentation boundary.
                        return Some(i);
                    } else {
                        expected_marker = (expected_marker + 1) % 8;
                    }
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }

        None
    }

    /// Check if a block of bytes has the high entropy characteristic of JPEG DCT compressed payload
    pub fn is_dct_payload(block: &[u8]) -> bool {
        let entropy = Self::calculate_entropy(block);
        // Valid compressed JPEG DCT data has entropy typically between 7.2 and 7.99
        entropy >= 7.2
    }
}
