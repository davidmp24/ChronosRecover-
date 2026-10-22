use byteorder::{ByteOrder, LittleEndian};

#[derive(Debug, Clone)]
pub struct NtfsVbr {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub cluster_size: u32,
    pub total_sectors: u64,
    pub mft_cluster: u64,
    pub mft_mirror_cluster: u64,
    pub mft_record_size: u32,
    pub volume_serial_number: u64,
}

impl NtfsVbr {
    pub fn parse(vbr_bytes: &[u8]) -> Option<Self> {
        if vbr_bytes.len() < 512 || !vbr_bytes.starts_with(b"\xEB\x52\x90NTFS ") {
            return None;
        }

        let bytes_per_sector = LittleEndian::read_u16(&vbr_bytes[11..13]) as u32;
        let sectors_per_cluster = vbr_bytes[13] as u32;
        if bytes_per_sector == 0 || sectors_per_cluster == 0 {
            return None;
        }

        let cluster_size = bytes_per_sector * sectors_per_cluster;
        let total_sectors = LittleEndian::read_u64(&vbr_bytes[40..48]);
        let mft_cluster = LittleEndian::read_u64(&vbr_bytes[48..56]);
        let mft_mirror_cluster = LittleEndian::read_u64(&vbr_bytes[56..64]);

        // Clusters per MFT record: signed byte
        let clusters_per_mft_raw = vbr_bytes[64] as i8;
        let mft_record_size = if clusters_per_mft_raw > 0 {
            (clusters_per_mft_raw as u32) * cluster_size
        } else {
            // Negative value represents 2^(-val) bytes, e.g. -10 => 2^10 = 1024 bytes
            1u32 << (clusters_per_mft_raw.abs() as u32)
        };

        let volume_serial = LittleEndian::read_u64(&vbr_bytes[72..80]);

        Some(Self {
            bytes_per_sector,
            sectors_per_cluster,
            cluster_size,
            total_sectors,
            mft_cluster,
            mft_mirror_cluster,
            mft_record_size,
            volume_serial_number: volume_serial,
        })
    }
}
