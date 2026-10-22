use crate::hal::error::{HalError, HalResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    PhysicalDisk,
    LogicalVolume,
    DiskImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusType {
    Unknown,
    Scsi,
    Atapi,
    Ata,
    Usb,
    Raid,
    Sas,
    Sata,
    Sd,
    Mmc,
    Nvme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveGeometry {
    pub bytes_per_sector: u32,
    pub bytes_per_physical_sector: u32,
    pub sectors_per_track: u32,
    pub tracks_per_cylinder: u32,
    pub cylinders: u64,
    pub total_sectors: u64,
    pub total_bytes: u64,
}

impl DriveGeometry {
    pub fn new(bytes_per_sector: u32, bytes_per_physical_sector: u32, total_bytes: u64) -> Self {
        let logical_sector = if bytes_per_sector == 0 { 512 } else { bytes_per_sector };
        let physical_sector = if bytes_per_physical_sector == 0 { 4096 } else { bytes_per_physical_sector };
        let total_sectors = total_bytes / (logical_sector as u64);
        Self {
            bytes_per_sector: logical_sector,
            bytes_per_physical_sector: physical_sector,
            sectors_per_track: 63,
            tracks_per_cylinder: 255,
            cylinders: total_sectors / (63 * 255),
            total_sectors,
            total_bytes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionDescriptor {
    pub index: u32,
    pub start_lba: u64,
    pub sector_count: u64,
    pub size_bytes: u64,
    pub fs_type: String,
    pub label: String,
    pub mount_point: Option<String>,
    pub volume_guid: Option<String>,
    pub is_orphan: bool,
    pub cluster_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskExtent {
    pub disk_number: u32,
    pub starting_offset: u64,
    pub extent_length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDescriptor {
    pub id: String,
    pub path: String,
    pub name: String,
    pub vendor: String,
    pub product: String,
    pub serial_number: String,
    pub device_type: DeviceType,
    pub bus_type: BusType,
    pub is_removable: bool,
    pub is_read_only: bool,
    pub geometry: DriveGeometry,
    pub partitions: Vec<PartitionDescriptor>,
    pub physical_drive_number: Option<u32>,
}

/// Core trait representing an accessible storage target (Physical Drive, Logical Volume, or Image)
pub trait StorageDevice: Send + Sync {
    /// Return static metadata and geometry of the device
    fn descriptor(&self) -> &DeviceDescriptor;

    /// Direct raw sector-aligned read with hardware watchdog timeout (3000ms)
    /// `lba`: starting Logical Block Address
    /// `sector_count`: number of sectors to read
    /// `buf`: output buffer whose length MUST equal `sector_count * geometry.bytes_per_sector`
    fn read_sectors(&self, lba: u64, sector_count: u64, buf: &mut [u8]) -> HalResult<usize>;

    /// General read at arbitrary byte offset (internally handles sector boundary alignment)
    fn read_at(&self, byte_offset: u64, buf: &mut [u8]) -> HalResult<usize>;

    /// Query whether device is still connected and accessible
    fn is_connected(&self) -> bool;

    /// Read volume disk extents to discover physical backing disks (used for Safe Recovery destination validation)
    fn get_disk_extents(&self) -> HalResult<Vec<DiskExtent>>;
}
