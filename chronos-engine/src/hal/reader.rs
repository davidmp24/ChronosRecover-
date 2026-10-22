use crate::hal::device::StorageDevice;
use crate::hal::error::{HalError, HalResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct BadSectorEvent {
    pub start_lba: u64,
    pub sector_count: u64,
    pub timestamp_ms: i64,
    pub error_message: String,
}

#[derive(Debug, Clone)]
pub struct ReaderStats {
    pub current_lba: u64,
    pub total_lba: u64,
    pub sectors_read: u64,
    pub bad_sectors_skipped: u64,
    pub bytes_read: u64,
    pub read_speed_mb_s: f64,
}

pub struct ResilientDiskReader {
    device: Arc<dyn StorageDevice>,
    start_lba: u64,
    end_lba: u64,
    current_lba: u64,
    chunk_sectors: u64,
    bad_sectors_count: u64,
    bytes_read_total: u64,
    is_cancelled: AtomicBool,
    start_time: Instant,
    last_speed_check: Instant,
    last_bytes_check: u64,
    current_speed_mb_s: f64,
}

impl ResilientDiskReader {
    pub fn new(
        device: Arc<dyn StorageDevice>,
        start_lba: u64,
        end_lba: Option<u64>,
        chunk_sectors: Option<u64>,
    ) -> Self {
        let total_sectors = device.descriptor().geometry.total_sectors;
        let final_lba = end_lba.unwrap_or(total_sectors).min(total_sectors);
        let chunk = chunk_sectors.unwrap_or(128); // 128 sectors * 512B = 64KB per batch

        Self {
            device,
            start_lba,
            end_lba: final_lba,
            current_lba: start_lba,
            chunk_sectors: chunk.max(8), // At least 4KB aligned
            bad_sectors_count: 0,
            bytes_read_total: 0,
            is_cancelled: AtomicBool::new(false),
            start_time: Instant::now(),
            last_speed_check: Instant::now(),
            last_bytes_check: 0,
            current_speed_mb_s: 0.0,
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_done(&self) -> bool {
        self.current_lba >= self.end_lba || self.is_cancelled.load(Ordering::Relaxed)
    }

    pub fn stats(&self) -> ReaderStats {
        ReaderStats {
            current_lba: self.current_lba,
            total_lba: self.end_lba,
            sectors_read: self.current_lba.saturating_sub(self.start_lba),
            bad_sectors_skipped: self.bad_sectors_count,
            bytes_read: self.bytes_read_total,
            read_speed_mb_s: self.current_speed_mb_s,
        }
    }

    /// Read next chunk of sectors into `buffer`.
    /// Returns `Ok(Some((lba, bytes_read)))` if successful,
    /// `Ok(None)` if end of range reached or cancelled,
    /// or `Err(HalError)` if device was completely disconnected.
    pub fn read_next_chunk<F>(
        &mut self,
        buffer: &mut Vec<u8>,
        mut on_bad_sector: F,
    ) -> HalResult<Option<(u64, usize)>>
    where
        F: FnMut(BadSectorEvent),
    {
        if self.is_done() {
            return Ok(None);
        }

        let sector_size = self.device.descriptor().geometry.bytes_per_sector as usize;
        let remaining_sectors = self.end_lba.saturating_sub(self.current_lba);
        let sectors_to_read = self.chunk_sectors.min(remaining_sectors);
        let expected_bytes = sectors_to_read as usize * sector_size;

        if buffer.len() < expected_bytes {
            buffer.resize(expected_bytes, 0);
        }

        let attempt_lba = self.current_lba;

        // Try reading the full chunk
        match self.device.read_sectors(attempt_lba, sectors_to_read, &mut buffer[..expected_bytes]) {
            Ok(bytes) => {
                self.current_lba += sectors_to_read;
                self.bytes_read_total += bytes as u64;
                self.update_telemetry();
                Ok(Some((attempt_lba, bytes)))
            }
            Err(HalError::BadSector { lba, os_code, details }) | Err(HalError::Timeout { lba }) => {
                // Corruption or hardware stall detected within this range.
                // Execute Adaptive Sector Skipping and zero-fill damaged buffer segments.
                let (bytes_recovered, skipped_sectors) = self.handle_corrupted_range(
                    attempt_lba,
                    sectors_to_read,
                    buffer,
                    &mut on_bad_sector,
                )?;

                self.current_lba += sectors_to_read;
                self.bad_sectors_count += skipped_sectors;
                self.bytes_read_total += bytes_recovered as u64;
                self.update_telemetry();

                Ok(Some((attempt_lba, expected_bytes)))
            }
            Err(HalError::DeviceDisconnected(msg)) => {
                error!(path = %self.device.descriptor().path, "Device disconnected abruptly during scan");
                Err(HalError::DeviceDisconnected(msg))
            }
            Err(HalError::EndOfDevice { .. }) => {
                info!("Reached End of Device at LBA {}", self.current_lba);
                self.current_lba = self.end_lba;
                Ok(None)
            }
            Err(e) => {
                warn!(lba = attempt_lba, error = ?e, "Unexpected I/O failure, skipping block");
                on_bad_sector(BadSectorEvent {
                    start_lba: attempt_lba,
                    sector_count: sectors_to_read,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    error_message: format!("{:?}", e),
                });
                self.current_lba += sectors_to_read;
                self.bad_sectors_count += sectors_to_read;
                buffer[..expected_bytes].fill(0);
                Ok(Some((attempt_lba, expected_bytes)))
            }
        }
    }

    /// Adaptive sector probe when a large chunk encounters bad sectors or timeouts
    fn handle_corrupted_range<F>(
        &self,
        start_lba: u64,
        total_sectors: u64,
        out_buf: &mut [u8],
        on_bad_sector: &mut F,
    ) -> HalResult<(usize, u64)>
    where
        F: FnMut(BadSectorEvent),
    {
        let sector_size = self.device.descriptor().geometry.bytes_per_sector as usize;
        let mut single_sector_buf = vec![0u8; sector_size];
        let mut bytes_recovered = 0usize;
        let mut skipped_sectors = 0u64;

        let mut offset_lba = 0u64;
        let mut current_skip_step = 1u64; // Exponential skip starting at 1 sector

        while offset_lba < total_sectors {
            let probe_lba = start_lba + offset_lba;
            let buf_offset = (offset_lba as usize) * sector_size;

            match self.device.read_sectors(probe_lba, 1, &mut single_sector_buf) {
                Ok(_) => {
                    // Good sector: copy to output buffer and reset skip step
                    out_buf[buf_offset..buf_offset + sector_size].copy_from_slice(&single_sector_buf);
                    bytes_recovered += sector_size;
                    offset_lba += 1;
                    current_skip_step = 1;
                }
                Err(HalError::BadSector { .. }) | Err(HalError::Timeout { .. }) => {
                    // Bad sector found: notify callback
                    let skip_len = current_skip_step.min(total_sectors - offset_lba);
                    let zero_end = (buf_offset + (skip_len as usize * sector_size)).min(out_buf.len());
                    out_buf[buf_offset..zero_end].fill(0);

                    on_bad_sector(BadSectorEvent {
                        start_lba: probe_lba,
                        sector_count: skip_len,
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                        error_message: format!("Adaptive Skip: {} sectors from LBA {}", skip_len, probe_lba),
                    });

                    skipped_sectors += skip_len;
                    offset_lba += skip_len;
                    // Exponential backoff for bad regions (1 -> 2 -> 4 -> 8 -> 16 -> 32 -> 64)
                    current_skip_step = (current_skip_step * 2).min(64);
                }
                Err(HalError::DeviceDisconnected(e)) => {
                    return Err(HalError::DeviceDisconnected(e));
                }
                Err(_) => {
                    // Other error: fill with zero and advance
                    out_buf[buf_offset..buf_offset + sector_size].fill(0);
                    offset_lba += 1;
                    skipped_sectors += 1;
                }
            }
        }

        Ok((bytes_recovered, skipped_sectors))
    }

    fn update_telemetry(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_speed_check);
        if elapsed.as_millis() >= 500 {
            let bytes_delta = self.bytes_read_total.saturating_sub(self.last_bytes_check);
            let mb_delta = (bytes_delta as f64) / (1024.0 * 1024.0);
            self.current_speed_mb_s = mb_delta / elapsed.as_secs_f64();

            self.last_speed_check = now;
            self.last_bytes_check = self.bytes_read_total;
        }
    }
}
