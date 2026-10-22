use byteorder::{ByteOrder, LittleEndian};

#[derive(Debug, Clone)]
pub struct Fat32Bpb {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub reserved_sectors: u32,
    pub num_fats: u32,
    pub total_sectors: u64,
    pub sectors_per_fat: u32,
    pub root_dir_cluster: u32,
    pub fs_info_sector: u32,
    pub backup_boot_sector: u32,
    pub cluster_size: u32,
    pub fat_start_sector: u64,
    pub data_start_sector: u64,
}

impl Fat32Bpb {
    pub fn parse(sector_0: &[u8]) -> Option<Self> {
        if sector_0.len() < 512 || sector_0[510] != 0x55 || sector_0[511] != 0xAA {
            return None;
        }

        let bytes_per_sector = LittleEndian::read_u16(&sector_0[11..13]) as u32;
        let sectors_per_cluster = sector_0[13] as u32;
        let reserved_sectors = LittleEndian::read_u16(&sector_0[14..16]) as u32;
        let num_fats = sector_0[16] as u32;

        if bytes_per_sector == 0 || sectors_per_cluster == 0 || num_fats == 0 {
            return None;
        }

        let total_sectors_16 = LittleEndian::read_u16(&sector_0[19..21]) as u64;
        let total_sectors_32 = LittleEndian::read_u32(&sector_0[32..36]) as u64;
        let total_sectors = if total_sectors_16 != 0 { total_sectors_16 } else { total_sectors_32 };

        let sectors_per_fat_16 = LittleEndian::read_u16(&sector_0[22..24]) as u32;
        let sectors_per_fat_32 = LittleEndian::read_u32(&sector_0[36..40]);
        let sectors_per_fat = if sectors_per_fat_16 != 0 { sectors_per_fat_16 } else { sectors_per_fat_32 };

        if sectors_per_fat == 0 {
            return None;
        }

        let root_dir_cluster = LittleEndian::read_u32(&sector_0[44..48]);
        let fs_info_sector = LittleEndian::read_u16(&sector_0[48..50]) as u32;
        let backup_boot_sector = LittleEndian::read_u16(&sector_0[50..52]) as u32;

        let cluster_size = bytes_per_sector * sectors_per_cluster;
        let fat_start_sector = reserved_sectors as u64;
        let data_start_sector = fat_start_sector + (num_fats as u64 * sectors_per_fat as u64);

        Some(Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            total_sectors,
            sectors_per_fat,
            root_dir_cluster,
            fs_info_sector,
            backup_boot_sector,
            cluster_size,
            fat_start_sector,
            data_start_sector,
        })
    }
}
