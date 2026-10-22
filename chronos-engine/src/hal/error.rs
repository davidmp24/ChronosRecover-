use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum HalError {
    #[error("Device not found or access denied: {0}")]
    DeviceNotFound(String),

    #[error("Permission denied (requires Administrator/root privileges): {0}")]
    PermissionDenied(String),

    #[error("Hardware read timeout expired (3000ms) on LBA {lba}")]
    Timeout { lba: u64 },

    #[error("Bad sector encountered at LBA {lba} (OS error {os_code:?}): {details}")]
    BadSector {
        lba: u64,
        os_code: Option<u32>,
        details: String,
    },

    #[error("Device disconnected abruptly or communication lost: {0}")]
    DeviceDisconnected(String),

    #[error("Unaligned sector buffer: buffer length {len} or offset {offset} is not a multiple of sector size {sector_size}")]
    UnalignedBuffer {
        len: usize,
        offset: u64,
        sector_size: u32,
    },

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("IOCTL failed: {0}")]
    IoctlError(String),

    #[error("End of device reached at LBA {lba}")]
    EndOfDevice { lba: u64 },
}

pub type HalResult<T> = Result<T, HalError>;
