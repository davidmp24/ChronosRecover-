use chronos_proto::recovery::DataRun;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedRun {
    pub lcn: Option<u64>, // None represents sparse run
    pub cluster_count: u64,
}

pub struct RunlistDecoder;

impl RunlistDecoder {
    /// Decodes raw NTFS Runlist bytes into absolute LCN cluster extents
    pub fn decode(runlist_bytes: &[u8]) -> Vec<DecodedRun> {
        let mut runs = Vec::new();
        let mut cursor = 0;
        let mut prev_lcn: i64 = 0;

        while cursor < runlist_bytes.len() {
            let header = runlist_bytes[cursor];
            cursor += 1;
            if header == 0 {
                // End of runlist marker
                break;
            }

            let len_size = (header & 0x0F) as usize;
            let offset_size = ((header >> 4) & 0x0F) as usize;

            if cursor + len_size + offset_size > runlist_bytes.len() {
                break;
            }

            // Read cluster count (unsigned)
            let mut count: u64 = 0;
            for i in 0..len_size {
                count |= (runlist_bytes[cursor + i] as u64) << (i * 8);
            }
            cursor += len_size;

            if offset_size == 0 {
                // Sparse cluster run
                runs.push(DecodedRun {
                    lcn: None,
                    cluster_count: count,
                });
            } else {
                // Read signed LCN delta
                let mut delta: i64 = 0;
                for i in 0..offset_size {
                    delta |= (runlist_bytes[cursor + i] as i64) << (i * 8);
                }
                // Sign extension
                let sign_bit = 1i64 << (offset_size * 8 - 1);
                if (delta & sign_bit) != 0 {
                    let mask = !0i64 << (offset_size * 8);
                    delta |= mask;
                }
                cursor += offset_size;

                prev_lcn = prev_lcn.wrapping_add(delta);
                let current_lcn = if prev_lcn >= 0 { prev_lcn as u64 } else { 0 };

                runs.push(DecodedRun {
                    lcn: Some(current_lcn),
                    cluster_count: count,
                });
            }
        }

        runs
    }

    pub fn to_proto_runs(runs: &[DecodedRun]) -> Vec<DataRun> {
        runs.iter()
            .map(|r| DataRun {
                cluster_offset: r.lcn.unwrap_or(0),
                cluster_count: r.cluster_count,
            })
            .collect()
    }
}
