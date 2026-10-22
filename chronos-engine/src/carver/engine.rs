use crate::carver::fragmentation::JpegFragmentAnalyzer;
use crate::carver::ring_buffer::{CarveBuffer, RingBufferPool};
use crate::carver::signatures::{FileSignature, SignatureCatalog};
use crate::hal::device::StorageDevice;
use crate::hal::reader::ResilientDiskReader;
use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use byteorder::{ByteOrder, BigEndian, LittleEndian};
use chronos_proto::recovery::{DiscoveredFile, FileCategory, FileStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tracing::{debug, info};
use uuid::Uuid;

pub struct CarvingEngine {
    signatures: Vec<FileSignature>,
    automaton: AhoCorasick,
    pool: Arc<RingBufferPool>,
    is_cancelled: Arc<AtomicBool>,
}

impl CarvingEngine {
    pub fn new() -> Self {
        let signatures = SignatureCatalog::get_mandatory_catalog();

        // Build SIMD-accelerated Aho-Corasick automaton across all signature headers
        let patterns: Vec<&[u8]> = signatures.iter().map(|s| s.header_magic).collect();
        let automaton = AhoCorasickBuilder::new()
            .ascii_case_insensitive(false)
            .build(&patterns)
            .expect("Failed to build Aho-Corasick automaton");

        let pool = Arc::new(RingBufferPool::new(8, 4 * 1024 * 1024));

        Self {
            signatures,
            automaton,
            pool,
            is_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
    }

    /// Run full Producer-Consumer carving pipeline over a device
    pub fn run_carve<F>(
        &self,
        device: Arc<dyn StorageDevice>,
        session_id: &str,
        start_lba: u64,
        end_lba: Option<u64>,
        mut on_file_found: F,
    ) where
        F: FnMut(DiscoveredFile) + Send + 'static,
    {
        let is_cancelled = self.is_cancelled.clone();
        let pool = self.pool.clone();
        let session_id_str = session_id.to_string();
        let sigs = self.signatures.clone();
        let automaton = self.automaton.clone();

        let (result_tx, result_rx) = crossbeam_channel::unbounded();

        // 1. Thread Producer (Sequential I/O Reader)
        let reader_device = device.clone();
        let reader_pool = pool.clone();
        let reader_cancelled = is_cancelled.clone();

        let producer_handle = thread::spawn(move || {
            let mut reader = ResilientDiskReader::new(
                reader_device.clone(),
                start_lba,
                end_lba,
                Some((reader_pool.buffer_size() / 512) as u64),
            );

            while !reader.is_done() && !reader_cancelled.load(Ordering::Relaxed) {
                let mut buf = match reader_pool.acquire_empty() {
                    Ok(b) => b,
                    Err(_) => break,
                };

                let lba_before = reader.stats().current_lba;
                match reader.read_next_chunk(&mut buf.data, |_bad_ev| {}) {
                    Ok(Some((lba, bytes))) => {
                        buf.lba_start = lba;
                        buf.valid_bytes = bytes;
                        if reader_pool.submit_filled(buf).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = reader_pool.release_buffer(buf);
                        break;
                    }
                    Err(_) => {
                        let _ = reader_pool.release_buffer(buf);
                        break;
                    }
                }
            }
        });

        // 2. Thread Consumer (SIMD Carver Workers)
        let worker_pool = pool.clone();
        let worker_cancelled = is_cancelled.clone();

        let consumer_handle = thread::spawn(move || {
            let rx = worker_pool.get_worker_receiver();
            let recycler = worker_pool.get_recycler_sender();

            while !worker_cancelled.load(Ordering::Relaxed) {
                let buffer = match rx.recv() {
                    Ok(b) => b,
                    Err(_) => break,
                };

                let slice = &buffer.data[..buffer.valid_bytes];

                // SIMD Aho-Corasick linear multi-pattern search
                for mat in automaton.find_iter(slice) {
                    let sig_idx = mat.pattern().as_usize();
                    let sig = &sigs[sig_idx];
                    let match_start = mat.start().saturating_sub(sig.header_offset);

                    // Sector alignment check (headers start at sector boundaries in 99% of cases)
                    if match_start % 512 != 0 {
                        continue;
                    }

                    let file_lba = buffer.lba_start + (match_start as u64 / 512);

                    // Determine file size and validate content
                    if let Some(file_size) = Self::resolve_file_size(sig, &slice[match_start..]) {
                        let extension = Self::refine_extension(sig, &slice[match_start..match_start + file_size.min(1024)]);

                        let file = DiscoveredFile {
                            id: format!("carved-lba-{:08x}-{}", file_lba, Uuid::new_v4()),
                            session_id: session_id_str.clone(),
                            partition_index: 0,
                            path: format!("/[Carved RAW]/{}.{}", file_lba, extension),
                            filename: format!("carved_{:08X}.{}", file_lba, extension),
                            extension: extension.to_string(),
                            size_bytes: file_size as u64,
                            status: FileStatus::FileStatusCarved as i32,
                            category: sig.category as i32,
                            mime_type: sig.mime_type.to_string(),
                            created_at: 0,
                            modified_at: 0,
                            accessed_at: 0,
                            deleted_at: 0,
                            recoverability_score: 100,
                            is_directory: false,
                            data_runs: Vec::new(),
                            starting_lba: file_lba,
                            md5_hash: String::new(),
                        };

                        let _ = result_tx.send(file);
                    }
                }

                let _ = recycler.send(buffer);
            }
        });

        // Collect discovered carved files in caller's thread
        while let Ok(file) = result_rx.recv() {
            on_file_found(file);
        }

        let _ = producer_handle.join();
        let _ = consumer_handle.join();
    }

    /// Calculate file size using either footer signatures or container length headers
    fn resolve_file_size(sig: &FileSignature, data: &[u8]) -> Option<usize> {
        if let Some(footer) = sig.footer_magic {
            // Find footer signature within max_size limit
            let max_search = data.len().min(sig.max_size as usize);
            if let Some(pos) = data[..max_search].windows(footer.len()).position(|w| w == footer) {
                return Some(pos + footer.len());
            }
            return None;
        }

        // Specific container formats with length in header:
        match sig.extension {
            "webp" | "avi" | "wav" => {
                // RIFF container: bytes 4..8 indicate file size minus 8
                if data.len() >= 8 && &data[0..4] == b"RIFF" {
                    let riff_len = LittleEndian::read_u32(&data[4..8]) as usize;
                    return Some((riff_len + 8).min(data.len()));
                }
            }
            "mp4" => {
                // Parse top-level ISO atoms (ftyp, moov, mdat)
                let mut offset = 0;
                while offset + 8 <= data.len() {
                    let atom_len = BigEndian::read_u32(&data[offset..offset + 4]) as usize;
                    if atom_len < 8 {
                        break;
                    }
                    offset += atom_len;
                    if offset >= data.len() || offset >= sig.max_size as usize {
                        break;
                    }
                }
                if offset > 8 {
                    return Some(offset);
                }
            }
            "sqlite" => {
                // SQLite header offset 16..18: page size; offset 28..32: database size in pages
                if data.len() >= 32 {
                    let page_size_raw = BigEndian::read_u16(&data[16..18]) as usize;
                    let page_size = if page_size_raw == 1 { 65536 } else { page_size_raw };
                    let page_count = BigEndian::read_u32(&data[28..32]) as usize;
                    if page_size > 0 && page_count > 0 {
                        let total = page_size * page_count;
                        return Some(total.min(data.len()));
                    }
                }
            }
            _ => {}
        }

        None
    }

    /// Refine extension for generic formats like ZIP (detect DOCX, XLSX, PPTX)
    fn refine_extension<'a>(sig: &'a FileSignature, data: &[u8]) -> &'a str {
        if sig.extension == "zip" {
            // Check for Office OpenXML markers: [Content_Types].xml or word/ / xl/ / ppt/
            let probe_len = data.len().min(2048);
            let probe_slice = &data[..probe_len];
            if probe_slice.windows(20).any(|w| w == b"[Content_Types].xml") {
                if probe_slice.windows(5).any(|w| w == b"word/") {
                    return "docx";
                } else if probe_slice.windows(3).any(|w| w == b"xl/") {
                    return "xlsx";
                } else if probe_slice.windows(4).any(|w| w == b"ppt/") {
                    return "pptx";
                }
            }
        }
        sig.extension
    }
}
