use crate::hal::device::DeviceDescriptor;
use crate::hal::reader::BadSectorEvent;
use chronos_proto::recovery::*;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::path::Path;
use thiserror::Error;
use tracing::{debug, info};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    DbError(#[from] rusqlite::Error),

    #[error("Connection pool error: {0}")]
    PoolError(#[from] r2d2::Error),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Clone)]
pub struct ForensicDatabase {
    pool: Pool<SqliteConnectionManager>,
}

impl ForensicDatabase {
    /// Initialize or open the SQLite forensic database with WAL mode enabled
    pub fn open<P: AsRef<Path>>(db_path: P) -> StorageResult<Self> {
        let manager = SqliteConnectionManager::file(db_path);
        let pool = Pool::builder()
            .max_size(16)
            .build(manager)?;

        let conn = pool.get()?;
        let schema_sql = include_str!("../../schema.sql");
        conn.execute_batch(schema_sql)?;

        info!("Forensic SQLite Database initialized in WAL mode");
        Ok(Self { pool })
    }

    /// Create an in-memory database for fast deterministic testing
    pub fn memory() -> StorageResult<Self> {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder()
            .max_size(8)
            .build(manager)?;

        let conn = pool.get()?;
        let schema_sql = include_str!("../../schema.sql");
        conn.execute_batch(schema_sql)?;

        Ok(Self { pool })
    }

    /// Register or update device metadata
    pub fn register_device(&self, dev: &DeviceDescriptor) -> StorageResult<()> {
        let conn = self.pool.get()?;
        let now = chrono::Utc::now().timestamp();
        let geom_hash = format!("{:016x}", dev.geometry.total_bytes);

        conn.execute(
            r#"
            INSERT INTO devices (
                id, path, name, vendor, product, serial_number,
                device_type, bus_type, is_removable, bytes_per_sector,
                bytes_per_physical_sector, total_sectors, total_bytes,
                geometry_hash, first_seen_at, last_seen_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
            ON CONFLICT(id) DO UPDATE SET
                path = excluded.path,
                last_seen_at = excluded.last_seen_at
            "#,
            params![
                dev.id,
                dev.path,
                dev.name,
                dev.vendor,
                dev.product,
                dev.serial_number,
                dev.device_type as i32,
                dev.bus_type as i32,
                dev.is_removable,
                dev.geometry.bytes_per_sector,
                dev.geometry.bytes_per_physical_sector,
                dev.geometry.total_sectors as i64,
                dev.geometry.total_bytes as i64,
                geom_hash,
                now,
            ],
        )?;

        Ok(())
    }

    /// Create a new scan session
    pub fn create_session(
        &self,
        device_id: &str,
        scan_mode: ScanMode,
        start_lba: u64,
        end_lba: u64,
    ) -> StorageResult<String> {
        let conn = self.pool.get()?;
        let session_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            r#"
            INSERT INTO sessions (
                id, device_id, scan_mode, status, start_lba, end_lba,
                current_lba, bad_sectors_count, files_discovered_count,
                history_events_count, elapsed_seconds, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0, 0, ?8, ?8)
            "#,
            params![
                session_id,
                device_id,
                scan_mode as i32,
                ScanStatus::ScanStatusRunning as i32,
                start_lba as i64,
                end_lba as i64,
                start_lba as i64,
                now,
            ],
        )?;

        Ok(session_id)
    }

    /// Record bad sector event detected during hardware read
    pub fn record_bad_sector(
        &self,
        session_id: &str,
        device_id: &str,
        event: &BadSectorEvent,
    ) -> StorageResult<()> {
        let conn = self.pool.get()?;
        conn.execute(
            r#"
            INSERT INTO bad_sectors (
                session_id, device_id, start_lba, sector_count,
                detected_at, error_message
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                session_id,
                device_id,
                event.start_lba as i64,
                event.sector_count as i64,
                event.timestamp_ms,
                event.error_message,
            ],
        )?;

        conn.execute(
            r#"
            UPDATE sessions 
            SET bad_sectors_count = bad_sectors_count + ?2,
                updated_at = ?3
            WHERE id = ?1
            "#,
            params![
                session_id,
                event.sector_count as i64,
                chrono::Utc::now().timestamp()
            ],
        )?;

        Ok(())
    }

    /// Update session scan progress checkpoint
    pub fn update_progress(
        &self,
        session_id: &str,
        current_lba: u64,
        files_count: u64,
        history_count: u64,
        elapsed_sec: u64,
        status: ScanStatus,
    ) -> StorageResult<()> {
        let conn = self.pool.get()?;
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            r#"
            UPDATE sessions 
            SET current_lba = ?2,
                files_discovered_count = ?3,
                history_events_count = ?4,
                elapsed_seconds = ?5,
                status = ?6,
                updated_at = ?7
            WHERE id = ?1
            "#,
            params![
                session_id,
                current_lba as i64,
                files_count as i64,
                history_count as i64,
                elapsed_sec as i64,
                status as i32,
                now,
            ],
        )?;

        Ok(())
    }

    /// Record discovered file into catalog
    pub fn insert_file(&self, file: &DiscoveredFile) -> StorageResult<()> {
        let conn = self.pool.get()?;
        let data_runs_json = serde_json::to_string(&file.data_runs)?;

        conn.execute(
            r#"
            INSERT INTO discovered_files (
                id, session_id, partition_id, path, filename, extension,
                size_bytes, status, category, mime_type, created_at,
                modified_at, accessed_at, deleted_at, recoverability_score,
                is_directory, starting_lba, data_runs_json, sha256_hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
            params![
                file.id,
                file.session_id,
                if file.partition_index == 0 { None::<String> } else { Some(format!("part-{}", file.partition_index)) },
                file.path,
                file.filename,
                file.extension,
                file.size_bytes as i64,
                file.status,
                file.category,
                file.mime_type,
                file.created_at,
                file.modified_at,
                file.accessed_at,
                file.deleted_at,
                file.recoverability_score as i32,
                file.is_directory,
                file.starting_lba as i64,
                data_runs_json,
                file.md5_hash,
            ],
        )?;

        Ok(())
    }

    /// Record timeline event from $UsnJrnl / $LogFile
    pub fn insert_history_event(&self, event: &HistoryEvent, device_id: &str) -> StorageResult<()> {
        let conn = self.pool.get()?;
        conn.execute(
            r#"
            INSERT INTO device_history (
                id, session_id, device_id, timestamp, event_type,
                source_path, destination_path, usn, file_reference_number,
                reason_flags, details
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                event.id,
                event.session_id,
                device_id,
                event.timestamp,
                event.event_type,
                event.source_path,
                event.destination_path,
                event.usn as i64,
                event.file_reference_number as i64,
                event.reason_flags,
                event.details,
            ],
        )?;

        Ok(())
    }

    /// Query discovered files with virtual pagination and filters
    pub fn query_files(&self, req: &FilterRequest) -> StorageResult<PaginatedFileResponse> {
        let conn = self.pool.get()?;
        let page_size = if req.page_size == 0 { 50 } else { req.page_size.min(1000) };
        let offset = req.page_number * page_size;

        let mut count_query = "SELECT COUNT(*) FROM discovered_files WHERE session_id = ?1".to_string();
        let mut data_query = "SELECT id, session_id, partition_id, path, filename, extension, size_bytes, status, category, mime_type, created_at, modified_at, accessed_at, deleted_at, recoverability_score, is_directory, starting_lba, data_runs_json, sha256_hash FROM discovered_files WHERE session_id = ?1".to_string();

        if !req.query.is_empty() {
            let filter = format!(" AND (filename LIKE '%{}%' OR path LIKE '%{}%')", req.query, req.query);
            count_query.push_str(&filter);
            data_query.push_str(&filter);
        }

        let total_matching: u64 = conn.query_row(&count_query, params![req.session_id], |row| row.get(0))?;

        let sort_order = if req.sort_descending { "DESC" } else { "ASC" };
        let order_col = match req.sort_by.as_str() {
            "size" => "size_bytes",
            "modified" => "modified_at",
            "recoverability" => "recoverability_score",
            _ => "filename",
        };

        data_query.push_str(&format!(" ORDER BY {} {} LIMIT {} OFFSET {}", order_col, sort_order, page_size, offset));

        let mut stmt = conn.prepare(&data_query)?;
        let rows = stmt.query_map(params![req.session_id], |row| {
            let runs_json: Option<String> = row.get(17)?;
            let runs: Vec<DataRun> = runs_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default();

            Ok(DiscoveredFile {
                id: row.get(0)?,
                session_id: row.get(1)?,
                partition_index: 0,
                path: row.get(3)?,
                filename: row.get(4)?,
                extension: row.get(5)?,
                size_bytes: row.get::<_, i64>(6)? as u64,
                status: row.get(7)?,
                category: row.get(8)?,
                mime_type: row.get(9)?,
                created_at: row.get(10)?,
                modified_at: row.get(11)?,
                accessed_at: row.get(12)?,
                deleted_at: row.get(13)?,
                recoverability_score: row.get::<_, i32>(14)? as u32,
                is_directory: row.get(15)?,
                data_runs: runs,
                starting_lba: row.get::<_, i64>(16)? as u64,
                md5_hash: row.get(18)?,
            })
        })?;

        let mut files = Vec::new();
        for r in rows {
            files.push(r?);
        }

        let total_pages = ((total_matching as f64) / (page_size as f64)).ceil() as u32;

        Ok(PaginatedFileResponse {
            files,
            total_matching_files: total_matching,
            current_page: req.page_number,
            total_pages,
        })
    }
}
