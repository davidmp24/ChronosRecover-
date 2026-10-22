use crate::hal::device::{
    BusType, DeviceDescriptor, DeviceType, DiskExtent, DriveGeometry, StorageDevice,
};
use crate::hal::error::{HalError, HalResult};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct ImageStorageDevice {
    file: RwLock<File>,
    descriptor: DeviceDescriptor,
    is_connected_flag: AtomicBool,
    /// Injected simulated bad sector LBAs for deterministic test suites
    injected_bad_sectors: RwLock<HashSet<u64>>,
}

impl ImageStorageDevice {
    pub fn open<P: AsRef<Path>>(path: P) -> HalResult<Self> {
        let path_ref = path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .open(path_ref)
            .map_err(|e| HalError::DeviceNotFound(format!("Failed to open image {:?}: {}", path_ref, e)))?;

        let total_bytes = file
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0);

        let geometry = DriveGeometry::new(512, 4096, total_bytes);
        let path_str = path_ref.to_string_lossy().to_string();

        let descriptor = DeviceDescriptor {
            id: format!("dev-img-{:016x}", total_bytes),
            path: path_str.clone(),
            name: format!("Forensic Image ({})", path_ref.file_name().unwrap_or_default().to_string_lossy()),
            vendor: "Forensic".to_string(),
            product: "Raw Disk Image".to_string(),
            serial_number: "IMAGE-VIRTUAL".to_string(),
            device_type: DeviceType::DiskImage,
            bus_type: BusType::Unknown,
            is_removable: false,
            is_read_only: true,
            geometry,
            partitions: Vec::new(),
            physical_drive_number: None,
        };

        Ok(Self {
            file: RwLock::new(file),
            descriptor,
            is_connected_flag: AtomicBool::new(true),
            injected_bad_sectors: RwLock::new(HashSet::new()),
        })
    }

    /// Inject simulated bad sectors for resilient testing of skipping algorithms
    pub fn inject_bad_sector(&self, lba: u64) {
        self.injected_bad_sectors.write().insert(lba);
    }

    pub fn clear_bad_sectors(&self) {
        self.injected_bad_sectors.write().clear();
    }
}

impl StorageDevice for ImageStorageDevice {
    fn descriptor(&self) -> &DeviceDescriptor {
        &self.descriptor
    }

    fn is_connected(&self) -> bool {
        self.is_connected_flag.load(Ordering::Relaxed)
    }

    fn read_sectors(&self, lba: u64, sector_count: u64, buf: &mut [u8]) -> HalResult<usize> {
        let sector_size = self.descriptor.geometry.bytes_per_sector as u64;
        let byte_offset = lba * sector_size;
        let expected_bytes = (sector_count * sector_size) as usize;

        if buf.len() < expected_bytes {
            return Err(HalError::UnalignedBuffer {
                len: buf.len(),
                offset: byte_offset,
                sector_size: self.descriptor.geometry.bytes_per_sector,
            });
        }

        // Check simulated bad sectors
        {
            let bad_set = self.injected_bad_sectors.read();
            for sector in lba..(lba + sector_count) {
                if bad_set.contains(&sector) {
                    return Err(HalError::BadSector {
                        lba: sector,
                        os_code: Some(23),
                        details: format!("Simulated CRC Bad Sector at LBA {}", sector),
                    });
                }
            }
        }

        let mut file_guard = self.file.write();
        file_guard
            .seek(SeekFrom::Start(byte_offset))
            .map_err(|e| HalError::IoError(format!("Seek failed to byte offset {}: {}", byte_offset, e)))?;

        let bytes_read = file_guard
            .read(&mut buf[..expected_bytes])
            .map_err(|e| HalError::IoError(format!("Read failed on image at LBA {}: {}", lba, e)))?;

        if bytes_read == 0 && expected_bytes > 0 {
            return Err(HalError::EndOfDevice { lba });
        }

        Ok(bytes_read)
    }

    fn read_at(&self, byte_offset: u64, buf: &mut [u8]) -> HalResult<usize> {
        let sector_size = self.descriptor.geometry.bytes_per_sector as u64;
        let start_lba = byte_offset / sector_size;
        let offset_in_sector = (byte_offset % sector_size) as usize;
        let end_offset = byte_offset + buf.len() as u64;
        let end_lba = (end_offset + sector_size - 1) / sector_size;
        let total_sectors = end_lba - start_lba;

        let mut aligned_buf = vec![0u8; (total_sectors * sector_size) as usize];
        self.read_sectors(start_lba, total_sectors, &mut aligned_buf)?;

        let available = aligned_buf.len().saturating_sub(offset_in_sector);
        let to_copy = buf.len().min(available);
        buf[..to_copy].copy_from_slice(&aligned_buf[offset_in_sector..offset_in_sector + to_copy]);

        Ok(to_copy)
    }

    fn get_disk_extents(&self) -> HalResult<Vec<DiskExtent>> {
        // Disk images do not belong to a physical drive index, return virtual extent
        Ok(vec![DiskExtent {
            disk_number: 9999,
            starting_offset: 0,
            extent_length: self.descriptor.geometry.total_bytes,
        }])
    }
}
