//! Segment merge policy and cleanup.

use std::path::{Path, PathBuf};

/// Decide which segments to merge.
///
/// Returns `None` if the segment count is within the limit. Otherwise returns
/// the ids of the smallest `merge_factor` segments.
pub fn select_segments_to_merge(
    sizes: &[(u64, usize)],
    max_segments: usize,
    merge_factor: usize,
) -> Option<Vec<u64>> {
    if sizes.len() <= max_segments {
        return None;
    }
    let mut indexed: Vec<(u64, usize)> = sizes.to_vec();
    indexed.sort_by_key(|a| a.1);
    Some(
        indexed
            .into_iter()
            .take(merge_factor)
            .map(|(id, _)| id)
            .collect(),
    )
}

/// List segment directories in `dir`.
pub fn list_segment_dirs(dir: impl AsRef<Path>) -> crate::Result<Vec<(u64, PathBuf)>> {
    let base = dir.as_ref();
    let mut out = Vec::new();
    if !base.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(id) = parse_segment_id(&name) {
            out.push((id, entry.path()));
        }
    }
    Ok(out)
}

/// Remove segment directories whose ids are not in `keep`.
pub fn remove_unused_segments(dir: impl AsRef<Path>, keep: &[u64]) -> crate::Result<()> {
    let keep: std::collections::HashSet<u64> = keep.iter().copied().collect();
    for (id, path) in list_segment_dirs(&dir)? {
        if !keep.contains(&id) {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
    Ok(())
}

fn parse_segment_id(name: &str) -> Option<u64> {
    if !name.starts_with("segment_") {
        return None;
    }
    u64::from_str_radix(&name[8..], 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_policy_selects_smallest() {
        let sizes = vec![(1, 100), (2, 50), (3, 75), (4, 30)];
        let selected = select_segments_to_merge(&sizes, 2, 2).unwrap();
        assert_eq!(selected, vec![4, 2]);
    }

    #[test]
    fn merge_policy_no_merge_when_under_limit() {
        let sizes = vec![(1, 100), (2, 50)];
        assert!(select_segments_to_merge(&sizes, 5, 2).is_none());
    }
}
