-- ChronosRecover High-Performance Forensic Storage Schema
-- Configured for SQLite WAL (Write-Ahead Logging) Mode with PRAGMA synchronous = NORMAL

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -64000; -- 64MB cache
PRAGMA foreign_keys = ON;

-- 1. Devices: Registered physical disks, volumes, or image files
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    vendor TEXT,
    product TEXT,
    serial_number TEXT,
    device_type INTEGER NOT NULL,
    bus_type INTEGER NOT NULL,
    is_removable BOOLEAN NOT NULL DEFAULT 0,
    bytes_per_sector INTEGER NOT NULL DEFAULT 512,
    bytes_per_physical_sector INTEGER NOT NULL DEFAULT 4096,
    total_sectors INTEGER NOT NULL,
    total_bytes INTEGER NOT NULL,
    geometry_hash TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
);

-- 2. Sessions: Scan sessions with Pause/Resume capability
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    scan_mode INTEGER NOT NULL,
    status INTEGER NOT NULL,
    start_lba INTEGER NOT NULL DEFAULT 0,
    end_lba INTEGER NOT NULL,
    current_lba INTEGER NOT NULL DEFAULT 0,
    bad_sectors_count INTEGER NOT NULL DEFAULT 0,
    files_discovered_count INTEGER NOT NULL DEFAULT 0,
    history_events_count INTEGER NOT NULL DEFAULT 0,
    elapsed_seconds INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    FOREIGN KEY(device_id) REFERENCES devices(id) ON DELETE CASCADE
);

-- 3. Bad Sectors: Defect map tracking for hardware resilience and skip algorithms
CREATE TABLE IF NOT EXISTS bad_sectors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    start_lba INTEGER NOT NULL,
    sector_count INTEGER NOT NULL,
    detected_at INTEGER NOT NULL,
    error_code INTEGER,
    error_message TEXT,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY(device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_bad_sectors_lookup 
ON bad_sectors(device_id, start_lba, sector_count);

-- 4. Partitions: Detected via MBR/GPT or discovered as orphan VBRs
CREATE TABLE IF NOT EXISTS partitions (
    id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    session_id TEXT,
    part_index INTEGER NOT NULL,
    start_lba INTEGER NOT NULL,
    sector_count INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    fs_type TEXT NOT NULL,
    label TEXT,
    mount_point TEXT,
    volume_guid TEXT,
    cluster_size INTEGER NOT NULL DEFAULT 4096,
    is_orphan BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_partitions_device 
ON partitions(device_id, start_lba);

-- 5. Discovered Files: High-scale file catalog (Active, Deleted, Orphan, Carved)
CREATE TABLE IF NOT EXISTS discovered_files (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    partition_id TEXT,
    parent_id TEXT,
    path TEXT NOT NULL,
    filename TEXT NOT NULL,
    extension TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    status INTEGER NOT NULL,              -- 0=Active, 1=Deleted, 2=Orphan, 3=Carved
    category INTEGER NOT NULL,            -- 0=Unknown, 1=Doc, 2=Img, 3=Vid, 4=Audio, etc.
    mime_type TEXT,
    created_at INTEGER,
    modified_at INTEGER,
    accessed_at INTEGER,
    deleted_at INTEGER,
    recoverability_score INTEGER NOT NULL DEFAULT 100, -- 0..100 based on $BITMAP collision check
    is_directory BOOLEAN NOT NULL DEFAULT 0,
    starting_lba INTEGER,
    first_cluster INTEGER,
    data_runs_json TEXT,                  -- JSON encoded array of cluster extents
    sha256_hash TEXT,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY(partition_id) REFERENCES partitions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_files_session_status 
ON discovered_files(session_id, status);

CREATE INDEX IF NOT EXISTS idx_files_session_category 
ON discovered_files(session_id, category);

CREATE INDEX IF NOT EXISTS idx_files_filename 
ON discovered_files(filename COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_files_path 
ON discovered_files(path COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_files_recoverability 
ON discovered_files(session_id, recoverability_score);

-- 6. Forensic Device History ($UsnJrnl, $LogFile, ext4 JBD2 timeline events)
CREATE TABLE IF NOT EXISTS device_history (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    event_type INTEGER NOT NULL,          -- 1=Created, 2=Deleted, 3=Renamed, etc.
    source_path TEXT,
    destination_path TEXT,
    usn INTEGER,
    file_reference_number INTEGER,
    reason_flags TEXT,
    details TEXT,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY(device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_history_session_ts 
ON device_history(session_id, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_history_path 
ON device_history(source_path COLLATE NOCASE, destination_path COLLATE NOCASE);

-- 7. Scan Checkpoints (for instant crash recovery and LBA resume)
CREATE TABLE IF NOT EXISTS scan_checkpoints (
    session_id TEXT PRIMARY KEY,
    last_lba INTEGER NOT NULL,
    bytes_read INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    checkpoint_data BLOB,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
