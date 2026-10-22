use chronos_proto::recovery::FileCategory;

#[derive(Debug, Clone)]
pub struct FileSignature {
    pub name: &'static str,
    pub extension: &'static str,
    pub category: FileCategory,
    pub mime_type: &'static str,
    pub header_magic: &'static [u8],
    pub footer_magic: Option<&'static [u8]>,
    pub max_size: u64,
    pub header_offset: usize,
}

pub struct SignatureCatalog;

impl SignatureCatalog {
    pub fn get_mandatory_catalog() -> Vec<FileSignature> {
        vec![
            // 1. JPEG / JPG
            FileSignature {
                name: "JPEG Image",
                extension: "jpg",
                category: FileCategory::FileCategoryImage,
                mime_type: "image/jpeg",
                header_magic: &[0xFF, 0xD8, 0xFF],
                footer_magic: Some(&[0xFF, 0xD9]),
                max_size: 50 * 1024 * 1024, // 50 MB
                header_offset: 0,
            },
            // 2. PNG
            FileSignature {
                name: "Portable Network Graphics",
                extension: "png",
                category: FileCategory::FileCategoryImage,
                mime_type: "image/png",
                header_magic: &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
                footer_magic: Some(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]),
                max_size: 100 * 1024 * 1024,
                header_offset: 0,
            },
            // 3. WebP (RIFF....WEBP)
            FileSignature {
                name: "WebP Image",
                extension: "webp",
                category: FileCategory::FileCategoryImage,
                mime_type: "image/webp",
                header_magic: &[0x52, 0x49, 0x46, 0x46], // Validated with WEBP at offset 8
                footer_magic: None,
                max_size: 50 * 1024 * 1024,
                header_offset: 0,
            },
            // 4. TIFF / RAW (CR2, NEF, ARW, DNG)
            FileSignature {
                name: "TIFF / Camera RAW (Little-Endian)",
                extension: "raw",
                category: FileCategory::FileCategoryImage,
                mime_type: "image/tiff",
                header_magic: &[0x49, 0x49, 0x2A, 0x00],
                footer_magic: None,
                max_size: 200 * 1024 * 1024,
                header_offset: 0,
            },
            FileSignature {
                name: "TIFF / Camera RAW (Big-Endian)",
                extension: "raw",
                category: FileCategory::FileCategoryImage,
                mime_type: "image/tiff",
                header_magic: &[0x4D, 0x4D, 0x00, 0x2A],
                footer_magic: None,
                max_size: 200 * 1024 * 1024,
                header_offset: 0,
            },
            // 5. MP4 / MOV (ISO Base Media)
            FileSignature {
                name: "MP4 Video",
                extension: "mp4",
                category: FileCategory::FileCategoryVideo,
                mime_type: "video/mp4",
                header_magic: b"ftyp",
                footer_magic: None,
                max_size: 10 * 1024 * 1024 * 1024, // 10 GB
                header_offset: 4,                  // 'ftyp' starts at byte 4 of atom
            },
            // 6. Matroska / WebM (1A 45 DF A3)
            FileSignature {
                name: "MKV / WebM Video",
                extension: "mkv",
                category: FileCategory::FileCategoryVideo,
                mime_type: "video/x-matroska",
                header_magic: &[0x1A, 0x45, 0xDF, 0xA3],
                footer_magic: None,
                max_size: 10 * 1024 * 1024 * 1024,
                header_offset: 0,
            },
            // 7. AVI Video (RIFF....AVI )
            FileSignature {
                name: "AVI Video",
                extension: "avi",
                category: FileCategory::FileCategoryVideo,
                mime_type: "video/x-msvideo",
                header_magic: b"AVI ",
                footer_magic: None,
                max_size: 4 * 1024 * 1024 * 1024,
                header_offset: 8,
            },
            // 8. MP3 Audio (ID3v2)
            FileSignature {
                name: "MP3 Audio (ID3)",
                extension: "mp3",
                category: FileCategory::FileCategoryAudio,
                mime_type: "audio/mpeg",
                header_magic: b"ID3",
                footer_magic: None,
                max_size: 100 * 1024 * 1024,
                header_offset: 0,
            },
            // 9. WAV Audio (RIFF....WAVE)
            FileSignature {
                name: "WAV Audio",
                extension: "wav",
                category: FileCategory::FileCategoryAudio,
                mime_type: "audio/wav",
                header_magic: b"WAVE",
                footer_magic: None,
                max_size: 1024 * 1024 * 1024,
                header_offset: 8,
            },
            // 10. PDF Document (%PDF)
            FileSignature {
                name: "PDF Document",
                extension: "pdf",
                category: FileCategory::FileCategoryDocument,
                mime_type: "application/pdf",
                header_magic: b"%PDF-",
                footer_magic: Some(b"%%EOF"),
                max_size: 500 * 1024 * 1024,
                header_offset: 0,
            },
            // 11. ZIP / DOCX / XLSX / PPTX (PK 03 04)
            FileSignature {
                name: "ZIP / Office OpenXML Document",
                extension: "zip",
                category: FileCategory::FileCategoryArchive,
                mime_type: "application/zip",
                header_magic: &[0x50, 0x4B, 0x03, 0x04],
                footer_magic: Some(&[0x50, 0x4B, 0x05, 0x06]), // End of Central Directory
                max_size: 2 * 1024 * 1024 * 1024,
                header_offset: 0,
            },
            // 12. RAR Archive (Rar!... 52 61 72 21 1A 07)
            FileSignature {
                name: "RAR Archive",
                extension: "rar",
                category: FileCategory::FileCategoryArchive,
                mime_type: "application/vnd.rar",
                header_magic: &[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07],
                footer_magic: None,
                max_size: 4 * 1024 * 1024 * 1024,
                header_offset: 0,
            },
            // 13. 7-Zip Archive (37 7A BC AF 27 1C)
            FileSignature {
                name: "7-Zip Archive",
                extension: "7z",
                category: FileCategory::FileCategoryArchive,
                mime_type: "application/x-7z-compressed",
                header_magic: &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C],
                footer_magic: None,
                max_size: 10 * 1024 * 1024 * 1024,
                header_offset: 0,
            },
            // 14. SQLite 3 Database ("SQLite format 3\0")
            FileSignature {
                name: "SQLite 3 Database",
                extension: "sqlite",
                category: FileCategory::FileCategoryDatabase,
                mime_type: "application/vnd.sqlite3",
                header_magic: b"SQLite format 3\0",
                footer_magic: None,
                max_size: 10 * 1024 * 1024 * 1024,
                header_offset: 0,
            },
        ]
    }
}
