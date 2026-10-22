#[cfg(unix)]
pub mod unix_hal {
    use crate::hal::device::{
        BusType, DeviceDescriptor, DeviceType, DiskExtent, DriveGeometry, StorageDevice,
    };
    use crate::hal::error::{HalError, HalResult};
    use nix::fcntl::{open, OFlag};
    use nix::sys::stat::Mode;
    use nix::unistd::{close, pread};
    use std::fs;
    use std::os::fd::RawFd;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tracing::{debug, info};

    pub struct LinuxStorageDevice {
        fd: RawFd,
        descriptor: DeviceDescriptor,
        is_connected_flag: AtomicBool,
    }

    impl Drop for LinuxStorageDevice {
        fn drop(&mut self) {
            if self.fd >= 0 {
                let _ = close(self.fd);
            }
        }
    }

    impl LinuxStorageDevice {
        pub fn open(device_path: &str) -> HalResult<Self> {
            let flags = OFlag::O_RDONLY | OFlag::O_DIRECT | OFlag::O_SYNC;
            let fd = open(Path::new(device_path), flags, Mode::empty())
                .map_err(|e| HalError::DeviceNotFound(format!("Failed to open {}: {}", device_path, e)))?;

            // Retrieve size and geometry via sysfs or lseek
            let total_bytes = fs::metadata(device_path)
                .map(|m| m.len())
                .unwrap_or(0);

            let geometry = DriveGeometry::new(512, 4096, total_bytes);

            let descriptor = DeviceDescriptor {
                id: format!("dev-linux-{}", device_path.replace('/', "_")),
                path: device_path.to_string(),
                name: format!("Linux Block Device ({})", device_path),
                vendor: "Linux".to_string(),
                product: "Block Device".to_string(),
                serial_number: "UNKNOWN".to_string(),
                device_type: DeviceType::PhysicalDisk,
                bus_type: BusType::Sata,
                is_removable: false,
                is_read_only: false,
                geometry,
                partitions: Vec::new(),
                physical_drive_number: None,
            };

            Ok(Self {
                fd,
                descriptor,
                is_connected_flag: AtomicBool::new(true),
            })
        }
    }

    impl StorageDevice for LinuxStorageDevice {
        fn descriptor(&self) -> &DeviceDescriptor {
            &self.descriptor
        }

        fn is_connected(&self) -> bool {
            self.is_connected_flag.load(Ordering::Relaxed)
        }

        fn read_sectors(&self, lba: u64, sector_count: u64, buf: &mut [u8]) -> HalResult<usize> {
            let sector_size = self.descriptor.geometry.bytes_per_sector as u64;
            let offset = (lba * sector_size) as i64;
            let expected_bytes = (sector_count * sector_size) as usize;

            if buf.len() < expected_bytes {
                return Err(HalError::UnalignedBuffer {
                    len: buf.len(),
                    offset: offset as u64,
                    sector_size: self.descriptor.geometry.bytes_per_sector,
                });
            }

            match pread(self.fd, &mut buf[..expected_bytes], offset) {
                Ok(bytes) => Ok(bytes),
                Err(nix::errno::Errno::EIO) => Err(HalError::BadSector {
                    lba,
                    os_code: Some(5),
                    details: "Linux Kernel EIO (Hardware I/O error)".into(),
                }),
                Err(nix::errno::Errno::ENODEV) => {
                    self.is_connected_flag.store(false, Ordering::Relaxed);
                    Err(HalError::DeviceDisconnected("Device detached".into()))
                }
                Err(e) => Err(HalError::IoError(format!("pread failed at LBA {}: {}", lba, e))),
            }
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
            Ok(vec![DiskExtent {
                disk_number: 0,
                starting_offset: 0,
                extent_length: self.descriptor.geometry.total_bytes,
            }])
        }
    }
}
