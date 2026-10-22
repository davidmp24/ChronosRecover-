pub mod device;
pub mod error;
pub mod image;
pub mod reader;

#[cfg(windows)]
pub mod windows;

#[cfg(unix)]
pub mod linux;

pub use device::*;
pub use error::*;
pub use image::*;
pub use reader::*;

use std::sync::Arc;
use std::path::Path;

/// Factory function to open any target storage device (Physical Disk, Volume, or Forensic Image)
pub fn open_device(path: &str) -> HalResult<Arc<dyn StorageDevice>> {
    // If path points to an existing file on disk, treat as forensic disk image
    if Path::new(path).is_file() {
        let img = ImageStorageDevice::open(path)?;
        return Ok(Arc::new(img));
    }

    #[cfg(windows)]
    {
        let win_dev = windows::win32::WindowsStorageDevice::open(path)?;
        return Ok(Arc::new(win_dev));
    }

    #[cfg(unix)]
    {
        let nix_dev = linux::unix_hal::LinuxStorageDevice::open(path)?;
        return Ok(Arc::new(nix_dev));
    }

    #[cfg(not(any(windows, unix)))]
    {
        Err(HalError::DeviceNotFound(format!("Platform not supported for raw path: {}", path)))
    }
}

/// Enumerate all detected physical and removable drives on the operating system
pub fn enumerate_devices() -> Vec<DeviceDescriptor> {
    #[cfg(windows)]
    {
        windows::win32::enumerate_windows_physical_drives()
    }

    #[cfg(unix)]
    {
        // Enumerate via /sys/block on Linux
        Vec::new()
    }

    #[cfg(not(any(windows, unix)))]
    {
        Vec::new()
    }
}
