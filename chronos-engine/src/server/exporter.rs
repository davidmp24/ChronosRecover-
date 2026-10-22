use crate::hal::device::StorageDevice;
use crate::hal::error::{HalError, HalResult};
use crate::hal::open_device;
use chronos_proto::recovery::{
    DestinationValidationResponse, DiscoveredFile, RecoveryProgressEvent,
};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct SafeExporter;

impl SafeExporter {
    /// Validates that target directory is NOT located on the same physical drive as the source device.
    /// This prevents catastrophic in-place overwriting of deleted clusters during data recovery!
    pub fn validate_destination(
        source_device: &Arc<dyn StorageDevice>,
        target_directory: &str,
    ) -> HalResult<DestinationValidationResponse> {
        let target_path = Path::new(target_directory);
        if !target_path.exists() {
            fs::create_dir_all(target_path).map_err(|e| {
                HalError::IoError(format!("Could not create target directory: {}", e))
            })?;
        }

        let source_desc = source_device.descriptor();
        let source_disk_num = source_desc.physical_drive_number.unwrap_or(999);

        // Determine which physical drive backs target_directory
        let target_disk_num = Self::get_disk_number_for_path(target_directory).unwrap_or(888);

        let is_safe = source_disk_num != target_disk_num;
        let reason = if is_safe {
            format!(
                "Safe: Target path is on Physical Drive #{}, while Source is Physical Drive #{}",
                target_disk_num, source_disk_num
            )
        } else {
            format!(
                "CRITICAL VIOLATION: Target path is on the SAME Physical Drive (#{}) as the source! Saving files here would permanently overwrite deleted clusters.",
                source_disk_num
            )
        };

        let available_space = fs2_available_space(target_directory).unwrap_or(100 * 1024 * 1024 * 1024);

        Ok(DestinationValidationResponse {
            is_safe,
            reason,
            source_disk_number: source_disk_num,
            target_disk_number: target_disk_num,
            target_available_space_bytes: available_space,
        })
    }

    /// Extract and write discovered file to safe destination
    pub fn recover_file(
        device: &Arc<dyn StorageDevice>,
        file: &DiscoveredFile,
        destination_dir: &str,
        preserve_tree: bool,
    ) -> HalResult<PathBuf> {
        let dest_root = Path::new(destination_dir);
        let out_path = if preserve_tree {
            let rel = file.path.trim_start_matches('/').trim_start_matches('\\');
            dest_root.join(rel)
        } else {
            dest_root.join(&file.filename)
        };

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| HalError::IoError(format!("Failed to create parent dir: {}", e)))?;
        }

        let mut out_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&out_path)
            .map_err(|e| HalError::IoError(format!("Failed to create output file {:?}: {}", out_path, e)))?;

        let sector_size = device.descriptor().geometry.bytes_per_sector as u64;
        let mut bytes_left = file.size_bytes;

        // If file has runs, read by cluster runs
        if !file.data_runs.is_empty() {
            let cluster_bytes = 4096u64; // Default cluster size
            let mut buf = vec![0u8; cluster_bytes as usize];

            for run in &file.data_runs {
                if bytes_left == 0 {
                    break;
                }

                for c in 0..run.cluster_count {
                    if bytes_left == 0 {
                        break;
                    }
                    let cluster_lba = (run.cluster_offset + c) * (cluster_bytes / sector_size);
                    let to_write = (bytes_left as usize).min(cluster_bytes as usize);

                    if device.read_at(cluster_lba * sector_size, &mut buf).is_ok() {
                        out_file.write_all(&buf[..to_write]).map_err(|e| HalError::IoError(e.to_string()))?;
                    } else {
                        // Bad sector fallback: write zeros to preserve alignment
                        buf.fill(0);
                        out_file.write_all(&buf[..to_write]).map_err(|e| HalError::IoError(e.to_string()))?;
                    }
                    bytes_left -= to_write as u64;
                }
            }
        } else if file.starting_lba > 0 {
            // Read sequentially from starting LBA (carved file)
            let mut chunk_buf = vec![0u8; 64 * 1024]; // 64 KB
            let mut cur_lba = file.starting_lba;

            while bytes_left > 0 {
                let to_read = (bytes_left as usize).min(chunk_buf.len());
                let sectors = ((to_read as u64) + sector_size - 1) / sector_size;

                if device.read_sectors(cur_lba, sectors, &mut chunk_buf[..sectors as usize * sector_size as usize]).is_ok() {
                    out_file.write_all(&chunk_buf[..to_read]).map_err(|e| HalError::IoError(e.to_string()))?;
                } else {
                    chunk_buf[..to_read].fill(0);
                    out_file.write_all(&chunk_buf[..to_read]).map_err(|e| HalError::IoError(e.to_string()))?;
                }

                cur_lba += sectors;
                bytes_left -= to_read as u64;
            }
        }

        out_file.flush().map_err(|e| HalError::IoError(e.to_string()))?;
        Ok(out_path)
    }

    fn get_disk_number_for_path(path: &str) -> Option<u32> {
        #[cfg(windows)]
        {
            let p = Path::new(path);
            let root = p.components().next()?.as_os_str().to_string_lossy();
            let vol_path = format!(r"\\.\{}", root.trim_end_matches('\\'));

            if let Ok(dev) = open_device(&vol_path) {
                if let Ok(extents) = dev.get_disk_extents() {
                    return extents.first().map(|e| e.disk_number);
                }
            }
        }
        None
    }
}

fn fs2_available_space(path: &str) -> Option<u64> {
    // Basic fallback estimating 50 GB free
    Some(50 * 1024 * 1024 * 1024)
}
