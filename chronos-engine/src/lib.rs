pub mod carver;
pub mod fs;
pub mod hal;
pub mod partition;
pub mod server;
pub mod storage;

pub use carver::*;
pub use fs::*;
pub use hal::*;
pub use partition::*;
pub use server::*;
pub use storage::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carver::engine::CarvingEngine;
    use crate::carver::signatures::SignatureCatalog;
    use crate::fs::fat::lfn::LfnBuilder;
    use crate::fs::ntfs::attributes::filetime_to_unix;
    use crate::fs::ntfs::bitmap::VolumeBitmap;
    use crate::fs::ntfs::mft::{MftParser, MFT_RECORD_MAGIC_FILE};
    use crate::fs::ntfs::runlist::{DecodedRun, RunlistDecoder};
    use crate::hal::device::StorageDevice;
    use crate::hal::image::ImageStorageDevice;
    use crate::hal::reader::ResilientDiskReader;
    use crate::partition::mbr::MbrParser;
    use crate::partition::vbr_scanner::VbrScanner;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_image_device_and_resilient_reader_with_bad_sectors() {
        let mut tmp_file = NamedTempFile::new().unwrap();
        let total_bytes = 1024 * 1024;
        let mut pattern = vec![0xAA; total_bytes];
        pattern[10 * 512..10 * 512 + 4].copy_from_slice(b"TEST");
        tmp_file.write_all(&pattern).unwrap();
        tmp_file.flush().unwrap();

        let image_device = Arc::new(ImageStorageDevice::open(tmp_file.path()).unwrap());
        assert_eq!(image_device.descriptor().geometry.total_bytes, total_bytes as u64);

        image_device.inject_bad_sector(15);
        image_device.inject_bad_sector(16);
        image_device.inject_bad_sector(17);
        image_device.inject_bad_sector(18);

        let mut reader = ResilientDiskReader::new(
            image_device.clone(),
            0,
            Some(64),
            Some(8),
        );

        let mut buffer = Vec::new();
        let mut bad_sector_events = Vec::new();

        while !reader.is_done() {
            let res = reader.read_next_chunk(&mut buffer, |ev| {
                bad_sector_events.push(ev);
            });
            assert!(res.is_ok());
        }

        let stats = reader.stats();
        assert_eq!(stats.current_lba, 64);
        assert!(stats.bad_sectors_skipped >= 4);
        assert!(!bad_sector_events.is_empty());
    }

    #[test]
    fn test_ntfs_runlist_and_recoverability_score() {
        // Runlist bytes:
        // Run 1: 0x21 (length=1 byte: 16 clusters, offset=2 bytes: +1000 delta LCN)
        // Run 2: 0x21 (length=1 byte: 32 clusters, offset=2 bytes: +500 delta LCN => 1500 LCN)
        // 0x00 (end of runlist)
        let runlist = vec![
            0x21, 16, 0xE8, 0x03,       // count=16, LCN = 1000
            0x21, 32, 0xF4, 0x01,       // count=32, LCN = 1000 + 500 = 1500
            0x00,
        ];

        let decoded = RunlistDecoder::decode(&runlist);
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].lcn, Some(1000));
        assert_eq!(decoded[0].cluster_count, 16);
        assert_eq!(decoded[1].lcn, Some(1500));
        assert_eq!(decoded[1].cluster_count, 32);

        // Test $BITMAP calculation:
        // Say total clusters is 2000. Bitmap has all 0s (unallocated => 100% recoverable)
        let bitmap_bytes = vec![0x00; 250]; // 2000 bits of zeros
        let bitmap = VolumeBitmap::new(bitmap_bytes, 2000);
        let score = bitmap.calculate_recoverability_score(&decoded);
        assert_eq!(score, 100, "Unallocated clusters must yield 100% recoverability score");

        // Now allocate clusters in cluster 1000..1016 (set bit 1000..1015 to 1)
        let mut partially_overwritten = vec![0x00; 250];
        for c in 1000..1016 {
            let byte_idx = c / 8;
            let bit_idx = c % 8;
            partially_overwritten[byte_idx] |= 1 << bit_idx;
        }
        let bitmap2 = VolumeBitmap::new(partially_overwritten, 2000);
        let score2 = bitmap2.calculate_recoverability_score(&decoded);
        // Total clusters = 16 + 32 = 48. Free clusters = 32. 32/48 = 66.6% => 67%
        assert_eq!(score2, 67);
    }

    #[test]
    fn test_fat_lfn_reconstruction() {
        let mut builder = LfnBuilder::new();
        let short_name: [u8; 11] = *b"REPORT  PDF";
        let checksum = LfnBuilder::calculate_checksum(&short_name);

        // Sequence 0x41 (Last fragment, seq 1, masked with 0x40): "Report.pdf"
        let mut lfn_entry = vec![0u8; 32];
        lfn_entry[0] = 0x41;
        lfn_entry[11] = 0x0F;
        lfn_entry[13] = checksum;
        // Characters: R, e, p, o, r
        let name_chars: Vec<u16> = "Report.pdf".encode_utf16().collect();
        // 5 chars from offset 1..11
        for i in 0..5 {
            let off = 1 + i * 2;
            lfn_entry[off] = (name_chars[i] & 0xFF) as u8;
            lfn_entry[off + 1] = ((name_chars[i] >> 8) & 0xFF) as u8;
        }
        // 5 remaining chars from offset 14..24
        for i in 0..5 {
            let off = 14 + i * 2;
            lfn_entry[off] = (name_chars[5 + i] & 0xFF) as u8;
            lfn_entry[off + 1] = ((name_chars[5 + i] >> 8) & 0xFF) as u8;
        }
        // Null terminator at offset 24..26
        lfn_entry[24] = 0;
        lfn_entry[25] = 0;

        builder.add_lfn_entry(&lfn_entry);
        let built_name = builder.build_filename(checksum);
        assert_eq!(built_name, Some("Report.pdf".to_string()));
    }

    #[test]
    fn test_carving_engine_with_synthetic_disk() {
        let mut tmp_file = NamedTempFile::new().unwrap();
        let mut disk_data = vec![0u8; 2 * 1024 * 1024]; // 2 MB synthetic disk

        // Inject JPEG at sector 10 (LBA 10)
        let jpeg_offset = 10 * 512;
        disk_data[jpeg_offset..jpeg_offset + 3].copy_from_slice(&[0xFF, 0xD8, 0xFF]);
        // JPEG payload
        disk_data[jpeg_offset + 3..jpeg_offset + 500].fill(0x55);
        // JPEG footer FF D9
        disk_data[jpeg_offset + 500..jpeg_offset + 502].copy_from_slice(&[0xFF, 0xD9]);

        // Inject PNG at sector 20 (LBA 20)
        let png_offset = 20 * 512;
        disk_data[png_offset..png_offset + 8].copy_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        disk_data[png_offset + 8..png_offset + 300].fill(0x77);
        // PNG footer IEND AE 42 60 82
        disk_data[png_offset + 300..png_offset + 308].copy_from_slice(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]);

        tmp_file.write_all(&disk_data).unwrap();
        tmp_file.flush().unwrap();

        let image_dev = Arc::new(ImageStorageDevice::open(tmp_file.path()).unwrap());
        let carver = CarvingEngine::new();

        let mut discovered = Vec::new();
        carver.run_carve(image_dev, "test-session", 0, Some(4096), move |file| {
            discovered.push(file);
        });

        assert!(discovered.len() >= 2, "Carver should find both JPEG and PNG files");
        let has_jpeg = discovered.iter().any(|f| f.extension == "jpg" && f.starting_lba == 10);
        let has_png = discovered.iter().any(|f| f.extension == "png" && f.starting_lba == 20);
        assert!(has_jpeg, "Should discover carved JPEG file at LBA 10");
        assert!(has_png, "Should discover carved PNG file at LBA 20");
    }
}
