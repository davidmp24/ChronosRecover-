use crate::fs::ntfs::runlist::DecodedRun;

#[derive(Debug, Clone)]
pub struct VolumeBitmap {
    bitmap_data: Vec<u8>,
    total_clusters: u64,
}

impl VolumeBitmap {
    pub fn new(bitmap_data: Vec<u8>, total_clusters: u64) -> Self {
        Self {
            bitmap_data,
            total_clusters,
        }
    }

    /// Check if a cluster is allocated (bit == 1) or free (bit == 0)
    pub fn is_cluster_allocated(&self, lcn: u64) -> bool {
        let byte_index = (lcn / 8) as usize;
        let bit_index = (lcn % 8) as u8;

        if byte_index >= self.bitmap_data.len() {
            // Beyond bitmap: assume allocated for safety
            return true;
        }

        (self.bitmap_data[byte_index] & (1 << bit_index)) != 0
    }

    /// Calculate Recoverability Score (0 to 100%) for a deleted file's runlist.
    /// If 100% of clusters remain unallocated (bit == 0), the data has NOT been overwritten!
    pub fn calculate_recoverability_score(&self, runs: &[DecodedRun]) -> u32 {
        let mut total_clusters = 0u64;
        let mut free_clusters = 0u64;

        for run in runs {
            let Some(start_lcn) = run.lcn else {
                continue; // Sparse run
            };

            for cluster in start_lcn..(start_lcn + run.cluster_count) {
                total_clusters += 1;
                if !self.is_cluster_allocated(cluster) {
                    free_clusters += 1;
                }
            }
        }

        if total_clusters == 0 {
            return 100; // Resident files or zero-byte files are 100% recoverable
        }

        let score = (free_clusters as f64 / total_clusters as f64) * 100.0;
        score.round() as u32
    }
}
