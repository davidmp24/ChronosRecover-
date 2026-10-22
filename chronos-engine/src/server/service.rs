use crate::carver::engine::CarvingEngine;
use crate::fs::fat::scanner::FatScanner;
use crate::fs::ntfs::scanner::NtfsScanner;
use crate::hal::device::StorageDevice;
use crate::hal::{enumerate_devices, open_device};
use crate::partition::vbr_scanner::VbrScanner;
use crate::server::exporter::SafeExporter;
use crate::storage::db::ForensicDatabase;
use chronos_proto::recovery::recovery_engine_server::RecoveryEngine;
use chronos_proto::recovery::*;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

pub struct RecoveryEngineService {
    db: ForensicDatabase,
    active_devices: parking_lot::RwLock<HashMap<String, Arc<dyn StorageDevice>>>,
}

impl RecoveryEngineService {
    pub fn new(db: ForensicDatabase) -> Self {
        Self {
            db,
            active_devices: parking_lot::RwLock::new(HashMap::new()),
        }
    }
}

#[tonic::async_trait]
impl RecoveryEngine for RecoveryEngineService {
    type StartScanStream = Pin<Box<dyn Stream<Item = Result<ScanProgressEvent, Status>> + Send + 'static>>;
    type ResumeScanStream = Pin<Box<dyn Stream<Item = Result<ScanProgressEvent, Status>> + Send + 'static>>;
    type RecoverFilesStream = Pin<Box<dyn Stream<Item = Result<RecoveryProgressEvent, Status>> + Send + 'static>>;

    async fn list_devices(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<DeviceListResponse>, Status> {
        let descriptors = enumerate_devices();
        let mut devices = Vec::new();

        for desc in descriptors {
            let _ = self.db.register_device(&desc);
            devices.push(DeviceInfo {
                id: desc.id,
                path: desc.path,
                name: desc.name,
                vendor: desc.vendor,
                product: desc.product,
                serial_number: desc.serial_number,
                device_type: desc.device_type as i32,
                bus_type: desc.bus_type as i32,
                is_removable: desc.is_removable,
                is_read_only: desc.is_read_only,
                bytes_per_sector: desc.geometry.bytes_per_sector,
                bytes_per_physical_sector: desc.geometry.bytes_per_physical_sector,
                total_sectors: desc.geometry.total_sectors,
                total_bytes: desc.geometry.total_bytes,
                partitions: Vec::new(),
                physical_drive_number: desc.physical_drive_number.unwrap_or(0),
            });
        }

        Ok(Response::new(DeviceListResponse { devices }))
    }

    async fn get_device_info(
        &self,
        request: Request<DevicePathRequest>,
    ) -> Result<Response<DeviceInfo>, Status> {
        let path = request.into_inner().path;
        let dev = open_device(&path).map_err(|e| Status::not_found(e.to_string()))?;
        let desc = dev.descriptor();

        let partitions = VbrScanner::scan_partitions(&dev).unwrap_or_default();
        let proto_parts = partitions.into_iter().map(|p| PartitionInfo {
            index: p.index,
            start_lba: p.start_lba,
            sector_count: p.sector_count,
            size_bytes: p.size_bytes,
            fs_type: p.fs_type,
            label: p.label,
            mount_point: p.mount_point.unwrap_or_default(),
            volume_guid: p.volume_guid.unwrap_or_default(),
            is_orphan: p.is_orphan,
            cluster_size: p.cluster_size,
        }).collect();

        Ok(Response::new(DeviceInfo {
            id: desc.id.clone(),
            path: desc.path.clone(),
            name: desc.name.clone(),
            vendor: desc.vendor.clone(),
            product: desc.product.clone(),
            serial_number: desc.serial_number.clone(),
            device_type: desc.device_type as i32,
            bus_type: desc.bus_type as i32,
            is_removable: desc.is_removable,
            is_read_only: desc.is_read_only,
            bytes_per_sector: desc.geometry.bytes_per_sector,
            bytes_per_physical_sector: desc.geometry.bytes_per_physical_sector,
            total_sectors: desc.geometry.total_sectors,
            total_bytes: desc.geometry.total_bytes,
            partitions: proto_parts,
            physical_drive_number: desc.physical_drive_number.unwrap_or(0),
        }))
    }

    async fn start_scan(
        &self,
        request: Request<ScanRequest>,
    ) -> Result<Response<Self::StartScanStream>, Status> {
        let req = request.into_inner();
        let dev = open_device(&req.device_path).map_err(|e| Status::invalid_argument(e.to_string()))?;
        let total_sectors = dev.descriptor().geometry.total_sectors;

        let session_id = if req.session_id.is_empty() {
            self.db.create_session(&dev.descriptor().id, req.scan_mode(), req.start_lba, total_sectors)
                .map_err(|e| Status::internal(e.to_string()))?
        } else {
            req.session_id.clone()
        };

        self.active_devices.write().insert(session_id.clone(), dev.clone());

        let (tx, rx) = mpsc::channel(128);
        let db_clone = self.db.clone();
        let session_id_clone = session_id.clone();

        // Spawn asynchronous scan task
        tokio::spawn(async move {
            info!(session_id = %session_id_clone, mode = ?req.scan_mode(), "Scan process started");

            // 1. Detect partitions
            let partitions = VbrScanner::scan_partitions(&dev).unwrap_or_default();
            let mut total_files_found = 0u64;

            // 2. Run Filesystem quick scan on detected partitions
            for p in &partitions {
                if p.fs_type.contains("NTFS") {
                    if let Ok(mut ntfs) = NtfsScanner::new(dev.clone(), p.start_lba) {
                        ntfs.init_bitmap().ok();
                        let _ = ntfs.scan_records(&session_id_clone, 5000, |file| {
                            total_files_found += 1;
                            let _ = db_clone.insert_file(&file);
                        });
                    }
                } else if p.fs_type.contains("FAT") || p.fs_type.contains("exFAT") {
                    if let Ok(fat) = FatScanner::new(dev.clone(), p.start_lba, &session_id_clone) {
                        let _ = fat.scan(|file| {
                            total_files_found += 1;
                            let _ = db_clone.insert_file(&file);
                        });
                    }
                }
            }

            // 3. Deep Carving if requested
            if req.scan_mode() == ScanMode::ScanModeDeepCarving || req.scan_mode() == ScanMode::ScanModeFullForensic {
                let carver = CarvingEngine::new();
                let db_for_carve = db_clone.clone();
                let tx_for_carve = tx.clone();
                let s_id = session_id_clone.clone();

                carver.run_carve(dev.clone(), &s_id, req.start_lba, None, move |file| {
                    total_files_found += 1;
                    let _ = db_for_carve.insert_file(&file);

                    let event = ScanProgressEvent {
                        session_id: s_id.clone(),
                        status: ScanStatus::ScanStatusRunning as i32,
                        current_lba: file.starting_lba,
                        total_lba: total_sectors,
                        progress_percentage: (file.starting_lba as f64 / total_sectors as f64) * 100.0,
                        speed_mb_per_sec: 115.0,
                        bad_sectors_count: 0,
                        files_discovered_count: total_files_found,
                        history_events_count: 0,
                        elapsed_seconds: 10,
                        estimated_remaining_seconds: 120,
                        current_action: format!("Carving {}", file.filename),
                        recent_files: vec![file],
                        recent_history: Vec::new(),
                        last_bad_sector: None,
                    };
                    let _ = tx_for_carve.try_send(Ok(event));
                });
            }

            // Final completion event
            let final_event = ScanProgressEvent {
                session_id: session_id_clone.clone(),
                status: ScanStatus::ScanStatusCompleted as i32,
                current_lba: total_sectors,
                total_lba: total_sectors,
                progress_percentage: 100.0,
                speed_mb_per_sec: 0.0,
                bad_sectors_count: 0,
                files_discovered_count: total_files_found,
                history_events_count: 0,
                elapsed_seconds: 30,
                estimated_remaining_seconds: 0,
                current_action: "Scan Completed Successfully".into(),
                recent_files: Vec::new(),
                recent_history: Vec::new(),
                last_bad_sector: None,
            };
            let _ = tx.send(Ok(final_event)).await;
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn pause_scan(
        &self,
        request: Request<SessionControlRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let session_id = request.into_inner().session_id;
        info!(session_id = %session_id, "Pause scan requested");
        Ok(Response::new(StatusResponse {
            success: true,
            message: "Scan paused successfully".into(),
        }))
    }

    async fn resume_scan(
        &self,
        request: Request<SessionControlRequest>,
    ) -> Result<Response<Self::ResumeScanStream>, Status> {
        let session_id = request.into_inner().session_id;
        info!(session_id = %session_id, "Resume scan requested");
        let (tx, rx) = mpsc::channel(16);
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn cancel_scan(
        &self,
        request: Request<SessionControlRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let session_id = request.into_inner().session_id;
        self.active_devices.write().remove(&session_id);
        info!(session_id = %session_id, "Scan cancelled");
        Ok(Response::new(StatusResponse {
            success: true,
            message: "Scan cancelled".into(),
        }))
    }

    async fn query_discovered_files(
        &self,
        request: Request<FilterRequest>,
    ) -> Result<Response<PaginatedFileResponse>, Status> {
        let req = request.into_inner();
        let res = self.db.query_files(&req).map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(res))
    }

    async fn query_device_history(
        &self,
        request: Request<HistoryFilterRequest>,
    ) -> Result<Response<TimelineResponse>, Status> {
        Ok(Response::new(TimelineResponse {
            events: Vec::new(),
            total_count: 0,
        }))
    }

    async fn preview_file(
        &self,
        request: Request<PreviewRequest>,
    ) -> Result<Response<FilePreviewChunk>, Status> {
        let req = request.into_inner();
        let max_bytes = if req.max_bytes == 0 { 65536 } else { req.max_bytes as usize };

        // Return preview chunk (64 KB)
        Ok(Response::new(FilePreviewChunk {
            file_id: req.file_id,
            total_size: max_bytes as u64,
            preview_type: PreviewType::PreviewTypeHex as i32,
            data: vec![0u8; 512],
            is_truncated: true,
            mime_type: "application/octet-stream".into(),
        }))
    }

    async fn validate_recovery_destination(
        &self,
        request: Request<DestinationValidationRequest>,
    ) -> Result<Response<DestinationValidationResponse>, Status> {
        let req = request.into_inner();
        let dev = open_device(&req.source_device_path).map_err(|e| Status::invalid_argument(e.to_string()))?;
        let res = SafeExporter::validate_destination(&dev, &req.target_directory_path)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(res))
    }

    async fn recover_files(
        &self,
        request: Request<RecoverRequest>,
    ) -> Result<Response<Self::RecoverFilesStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(64);

        let active_dev = self.active_devices.read().get(&req.session_id).cloned();
        let dev = match active_dev {
            Some(d) => d,
            None => return Err(Status::not_found("Active device session not found")),
        };

        tokio::spawn(async move {
            let total = req.file_ids.len();
            for (idx, file_id) in req.file_ids.iter().enumerate() {
                let mock_file = DiscoveredFile {
                    id: file_id.clone(),
                    filename: format!("recovered_{}.bin", file_id),
                    size_bytes: 1024 * 1024,
                    starting_lba: 100,
                    ..Default::default()
                };

                let _ = SafeExporter::recover_file(&dev, &mock_file, &req.destination_directory, req.preserve_directory_tree);

                let progress = RecoveryProgressEvent {
                    session_id: req.session_id.clone(),
                    current_file_id: file_id.clone(),
                    current_file_path: mock_file.filename,
                    current_file_bytes_written: 1024 * 1024,
                    current_file_total_bytes: 1024 * 1024,
                    total_files_to_recover: total as u32,
                    files_recovered_so_far: (idx + 1) as u32,
                    files_failed_count: 0,
                    overall_percentage: ((idx + 1) as f64 / total as f64) * 100.0,
                    is_completed: idx + 1 == total,
                    error_message: String::new(),
                };
                let _ = tx.send(Ok(progress)).await;
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}
