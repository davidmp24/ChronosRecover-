#[cfg(windows)]
pub mod win32 {
    use crate::hal::device::{
        BusType, DeviceDescriptor, DeviceType, DiskExtent, DriveGeometry, PartitionDescriptor,
        StorageDevice,
    };
    use crate::hal::error::{HalError, HalResult};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tracing::{debug, error, info, warn};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_CRC,
        ERROR_DEVICE_NOT_CONNECTED, ERROR_HANDLE_EOF, ERROR_IO_PENDING, ERROR_NOT_READY,
        ERROR_OPERATION_ABORTED, ERROR_SECTOR_NOT_FOUND, GENERIC_READ, HANDLE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{
        DISK_GEOMETRY_EX, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
        IOCTL_DISK_GET_PARTITION_INFO_EX, IOCTL_STORAGE_QUERY_PROPERTY,
        IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, PARTITION_INFORMATION_EX,
        PARTITION_STYLE_GPT, PARTITION_STYLE_MBR, PropertyStandardQuery,
        STORAGE_BUS_TYPE, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
        StorageDeviceProperty, VOLUME_DISK_EXTENTS,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, ResetEvent, WaitForSingleObject,
    };
    use windows::Win32::System::IO::{
        CancelIoEx, DeviceIoControl, GetOverlappedResult, ReadFile, OVERLAPPED,
    };

    const HARDWARE_TIMEOUT_MS: u32 = 3000;

    fn to_wide_null(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub struct WindowsStorageDevice {
        handle: HANDLE,
        descriptor: DeviceDescriptor,
        is_connected_flag: AtomicBool,
    }

    // Safety: Windows HANDLE for an opened disk device can be safely shared across threads for read operations
    unsafe impl Send for WindowsStorageDevice {}
    unsafe impl Sync for WindowsStorageDevice {}

    impl Drop for WindowsStorageDevice {
        fn drop(&mut self) {
            if !self.handle.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.handle);
                }
            }
        }
    }

    impl WindowsStorageDevice {
        /// Open raw physical drive (e.g. `\\.\PhysicalDrive0`) or logical volume (e.g. `\\.\Volume{...}`)
        pub fn open(device_path: &str) -> HalResult<Self> {
            let wide_path = to_wide_null(device_path);
            let flags = FILE_FLAGS_AND_ATTRIBUTES(
                FILE_FLAG_OVERLAPPED.0 | FILE_FLAG_NO_BUFFERING.0,
            );

            let handle = unsafe {
                CreateFileW(
                    PCWSTR(wide_path.as_ptr()),
                    GENERIC_READ.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    flags,
                    HANDLE::default(),
                )
            };

            let handle = match handle {
                Ok(h) if !h.is_invalid() => h,
                _ => {
                    let err = unsafe { GetLastError() };
                    if err == ERROR_ACCESS_DENIED {
                        return Err(HalError::PermissionDenied(format!(
                            "Access denied opening {}. Please run ChronosRecover as Administrator.",
                            device_path
                        )));
                    }
                    return Err(HalError::DeviceNotFound(format!(
                        "Could not open device {}: Win32 Error 0x{:08X}",
                        device_path, err.0
                    )));
                }
            };

            // Query Disk Geometry via IOCTL_DISK_GET_DRIVE_GEOMETRY_EX
            let geometry = match Self::query_geometry(handle) {
                Ok(geom) => geom,
                Err(err) => {
                    unsafe { let _ = CloseHandle(handle); }
                    return Err(err);
                }
            };

            // Query Storage Hardware Properties (Vendor, Product, Serial, BusType)
            let (vendor, product, serial, bus_type, is_removable) =
                Self::query_storage_properties(handle).unwrap_or_else(|_| {
                    ("Generic".to_string(), "Storage Device".to_string(), "UNKNOWN".to_string(), BusType::Unknown, false)
                });

            let physical_drive_num = Self::extract_physical_number(device_path);
            let device_type = if device_path.to_ascii_lowercase().contains("physicaldrive") {
                DeviceType::PhysicalDisk
            } else {
                DeviceType::LogicalVolume
            };

            let name = if !vendor.is_empty() || !product.is_empty() {
                format!("{} {}", vendor.trim(), product.trim()).trim().to_string()
            } else {
                device_path.to_string()
            };

            let id = format!(
                "dev-win-{}-{:016x}",
                physical_drive_num.unwrap_or(0),
                geometry.total_bytes
            );

            let descriptor = DeviceDescriptor {
                id,
                path: device_path.to_string(),
                name,
                vendor,
                product,
                serial_number: serial,
                device_type,
                bus_type,
                is_removable,
                is_read_only: false,
                geometry,
                partitions: Vec::new(),
                physical_drive_number: physical_drive_num,
            };

            info!(
                path = %device_path,
                bytes = descriptor.geometry.total_bytes,
                sector_size = descriptor.geometry.bytes_per_sector,
                physical_sector = descriptor.geometry.bytes_per_physical_sector,
                "Win32 Raw Disk opened successfully"
            );

            Ok(Self {
                handle,
                descriptor,
                is_connected_flag: AtomicBool::new(true),
            })
        }

        fn query_geometry(handle: HANDLE) -> HalResult<DriveGeometry> {
            let mut geom_ex = DISK_GEOMETRY_EX::default();
            let mut bytes_returned = 0u32;

            let success = unsafe {
                DeviceIoControl(
                    handle,
                    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
                    None,
                    0,
                    Some(&mut geom_ex as *mut _ as *mut _),
                    std::mem::size_of::<DISK_GEOMETRY_EX>() as u32,
                    Some(&mut bytes_returned),
                    None,
                )
            };

            if let Err(err) = success {
                return Err(HalError::IoctlError(format!(
                    "IOCTL_DISK_GET_DRIVE_GEOMETRY_EX failed: {:?}",
                    err
                )));
            }

            let disk_size = unsafe { geom_ex.DiskSize };
            let logical_bytes = geom_ex.Geometry.BytesPerSector;
            let physical_bytes = 4096; // Typical modern physical sector alignment

            Ok(DriveGeometry::new(logical_bytes, physical_bytes, disk_size as u64))
        }

        fn query_storage_properties(
            handle: HANDLE,
        ) -> HalResult<(String, String, String, BusType, bool)> {
            let query = STORAGE_PROPERTY_QUERY {
                PropertyId: StorageDeviceProperty,
                QueryType: PropertyStandardQuery,
                AdditionalParameters: [0],
            };

            let mut buffer = vec![0u8; 1024];
            let mut bytes_returned = 0u32;

            let success = unsafe {
                DeviceIoControl(
                    handle,
                    IOCTL_STORAGE_QUERY_PROPERTY,
                    Some(&query as *const _ as *const _),
                    std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                    Some(buffer.as_mut_ptr() as *mut _),
                    buffer.len() as u32,
                    Some(&mut bytes_returned),
                    None,
                )
            };

            if let Err(err) = success {
                return Err(HalError::IoctlError(format!(
                    "IOCTL_STORAGE_QUERY_PROPERTY failed: {:?}",
                    err
                )));
            }

            if bytes_returned < std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32 {
                return Err(HalError::IoctlError("Buffer too small for descriptor".to_string()));
            }

            let desc = unsafe { &*(buffer.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };

            let extract_str = |offset: u32| -> String {
                if offset == 0 || (offset as usize) >= buffer.len() {
                    return String::new();
                }
                let slice = &buffer[offset as usize..];
                let len = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                String::from_utf8_lossy(&slice[..len]).trim().to_string()
            };

            let vendor = extract_str(desc.VendorIdOffset);
            let product = extract_str(desc.ProductIdOffset);
            let serial = extract_str(desc.SerialNumberOffset);
            let is_removable = desc.RemovableMedia.as_bool();

            let bus_type = match desc.BusType {
                STORAGE_BUS_TYPE(1) => BusType::Scsi,
                STORAGE_BUS_TYPE(2) => BusType::Atapi,
                STORAGE_BUS_TYPE(3) => BusType::Ata,
                STORAGE_BUS_TYPE(7) => BusType::Usb,
                STORAGE_BUS_TYPE(8) => BusType::Raid,
                STORAGE_BUS_TYPE(10) => BusType::Sas,
                STORAGE_BUS_TYPE(11) => BusType::Sata,
                STORAGE_BUS_TYPE(12) => BusType::Sd,
                STORAGE_BUS_TYPE(13) => BusType::Mmc,
                STORAGE_BUS_TYPE(17) => BusType::Nvme,
                _ => BusType::Unknown,
            };

            Ok((vendor, product, serial, bus_type, is_removable))
        }

        fn extract_physical_number(path: &str) -> Option<u32> {
            let lower = path.to_ascii_lowercase();
            if let Some(pos) = lower.find("physicaldrive") {
                let rest = &lower[pos + "physicaldrive".len()..];
                return rest.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok();
            }
            None
        }
    }

    impl StorageDevice for WindowsStorageDevice {
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

            let h_event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) };
            let h_event = match h_event {
                Ok(ev) if !ev.is_invalid() => ev,
                _ => return Err(HalError::IoError("Failed to create Win32 Event for Overlapped IO".into())),
            };

            // Set up OVERLAPPED with 64-bit file offset
            let mut overlapped = OVERLAPPED {
                Internal: 0,
                InternalHigh: 0,
                Anonymous: windows::Win32::System::IO::OVERLAPPED_0 {
                    Offset: (byte_offset & 0xFFFF_FFFF) as u32,
                    OffsetHigh: ((byte_offset >> 32) & 0xFFFF_FFFF) as u32,
                },
                hEvent: h_event,
            };

            let read_slice = &mut buf[..expected_bytes];
            let mut bytes_transferred = 0u32;

            // Initiate Asynchronous Overlapped Read
            let read_result = unsafe {
                ReadFile(
                    self.handle,
                    Some(read_slice.as_mut_ptr() as *mut _),
                    expected_bytes as u32,
                    Some(&mut bytes_transferred),
                    Some(&mut overlapped),
                )
            };

            let mut final_result = Ok(bytes_transferred as usize);

            if let Err(_) = read_result {
                let err = unsafe { GetLastError() };
                if err == ERROR_IO_PENDING {
                    // Wait with 3000ms Watchdog Timeout
                    let wait_res = unsafe { WaitForSingleObject(h_event, HARDWARE_TIMEOUT_MS) };
                    if wait_res == WAIT_TIMEOUT {
                        warn!(lba = lba, "Hardware Read Timeout (3000ms) expired. Cancelling I/O via CancelIoEx.");
                        unsafe {
                            let _ = CancelIoEx(self.handle, Some(&overlapped));
                            // Drain cancelled event
                            let _ = WaitForSingleObject(h_event, 500);
                        }
                        final_result = Err(HalError::Timeout { lba });
                    } else if wait_res == WAIT_OBJECT_0 {
                        let mut transferred = 0u32;
                        let ov_res = unsafe {
                            GetOverlappedResult(self.handle, &overlapped, &mut transferred, false)
                        };
                        if ov_res.is_ok() {
                            final_result = Ok(transferred as usize);
                        } else {
                            let ov_err = unsafe { GetLastError() };
                            final_result = Err(map_win32_read_error(ov_err.0, lba));
                        }
                    } else {
                        final_result = Err(HalError::IoError(format!(
                            "WaitForSingleObject failed on LBA {}: 0x{:08X}",
                            lba, wait_res.0
                        )));
                    }
                } else {
                    final_result = Err(map_win32_read_error(err.0, lba));
                }
            }

            unsafe {
                let _ = CloseHandle(h_event);
            }

            if let Err(ref e) = final_result {
                if matches!(e, HalError::DeviceDisconnected(_)) {
                    self.is_connected_flag.store(false, Ordering::Relaxed);
                }
            }

            final_result
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
            let mut extents_buffer = vec![0u8; 1024];
            let mut bytes_returned = 0u32;

            let res = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
                    None,
                    0,
                    Some(extents_buffer.as_mut_ptr() as *mut _),
                    extents_buffer.len() as u32,
                    Some(&mut bytes_returned),
                    None,
                )
            };

            if let Err(err) = res {
                return Err(HalError::IoctlError(format!(
                    "IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS failed: {:?}",
                    err
                )));
            }

            let volume_extents = unsafe { &*(extents_buffer.as_ptr() as *const VOLUME_DISK_EXTENTS) };
            let count = volume_extents.NumberOfDiskExtents as usize;

            let mut extents = Vec::with_capacity(count);
            for i in 0..count {
                let extent = volume_extents.Extents[i];
                extents.push(DiskExtent {
                    disk_number: extent.DiskNumber,
                    starting_offset: unsafe { extent.StartingOffset } as u64,
                    extent_length: unsafe { extent.ExtentLength } as u64,
                });
            }

            Ok(extents)
        }
    }

    fn map_win32_read_error(code: u32, lba: u64) -> HalError {
        match windows::Win32::Foundation::WIN32_ERROR(code) {
            ERROR_CRC | ERROR_SECTOR_NOT_FOUND => HalError::BadSector {
                lba,
                os_code: Some(code),
                details: format!("CRC/Bad Sector Hardware Read Failure at LBA {}", lba),
            },
            ERROR_DEVICE_NOT_CONNECTED | ERROR_NOT_READY => HalError::DeviceDisconnected(format!(
                "Device was removed or disconnected during read at LBA {}",
                lba
            )),
            ERROR_HANDLE_EOF => HalError::EndOfDevice { lba },
            ERROR_OPERATION_ABORTED => HalError::Timeout { lba },
            _ => HalError::IoError(format!("Win32 I/O Error 0x{:08X} at LBA {}", code, lba)),
        }
    }

    /// Enumerate all connected physical drives on Windows (PhysicalDrive0 to PhysicalDrive32)
    pub fn enumerate_windows_physical_drives() -> Vec<DeviceDescriptor> {
        let mut devices = Vec::new();
        for i in 0..32 {
            let path = format!(r"\\.\PhysicalDrive{}", i);
            match WindowsStorageDevice::open(&path) {
                Ok(dev) => {
                    devices.push(dev.descriptor().clone());
                }
                Err(HalError::DeviceNotFound(_)) => {
                    // Disk index does not exist, continue scan
                    continue;
                }
                Err(e) => {
                    debug!("Physical drive {} inspection error: {:?}", i, e);
                }
            }
        }
        devices
    }
}
