//! Manifest snapshot write/load/truncate helpers.
//!
//! A snapshot is a JSON-serialized `Manifest` plus its LSN. The `CURRENT` file
//! points to the latest snapshot name. After a snapshot is written and `CURRENT`
//! is updated, old WAL segments can be truncated.

use std::path::{Path, PathBuf};

use storage_file::atomic_write;

use crate::manifest::Manifest;
use crate::{Error, Result};

pub(crate) const CURRENT_FILE: &str = "CURRENT";
const SNAPSHOT_DIR: &str = "manifest-snapshot";

/// Write a snapshot of `manifest` at `lsn` and update the `CURRENT` pointer.
///
/// The snapshot and `CURRENT` pointer are both written atomically using
/// `storage_file::atomic_write`, which fsyncs the temporary file, renames it
/// over the destination, and fsyncs the parent directory.
pub fn write(path: &Path, manifest: &Manifest, lsn: u64) -> Result<PathBuf> {
    let snapshot_dir = path.join(SNAPSHOT_DIR);
    std::fs::create_dir_all(&snapshot_dir)?;

    let snapshot_name = format!("{:020}.snapshot", lsn);
    let snapshot_path = snapshot_dir.join(&snapshot_name);

    let json = serde_json::to_vec(manifest)?;
    atomic_write(&snapshot_path, &json)?;

    let current_path = path.join(CURRENT_FILE);
    atomic_write(&current_path, snapshot_name.as_bytes())?;

    Ok(snapshot_path)
}

/// Load the latest snapshot and its LSN.
///
/// Returns `Error::CorruptSnapshot` if `CURRENT` exists but the snapshot it
/// points to cannot be read or decoded. Callers that need to distinguish a
/// missing snapshot from a corrupt one should check for `CURRENT` first.
pub fn load(path: &Path) -> Result<(Manifest, u64)> {
    let current_path = path.join(CURRENT_FILE);
    let current = std::fs::read_to_string(&current_path).map_err(|e| {
        Error::CorruptSnapshot(format!("failed to read CURRENT: {e}"))
    })?;
    let snapshot_name = current.trim();
    let snapshot_path = path.join(SNAPSHOT_DIR).join(snapshot_name);
    let bytes = std::fs::read(&snapshot_path).map_err(|e| {
        Error::CorruptSnapshot(format!("failed to read snapshot file {snapshot_path:?}: {e}"))
    })?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| {
        Error::CorruptSnapshot(format!("failed to decode snapshot: {e}"))
    })?;

    let lsn = parse_lsn(snapshot_name)?;
    Ok((manifest, lsn))
}

fn parse_lsn(name: &str) -> Result<u64> {
    let stem = name.strip_suffix(".snapshot").ok_or_else(|| {
        Error::CorruptSnapshot(format!("snapshot name missing .snapshot suffix: {name}"))
    })?;
    stem.parse::<u64>()
        .map_err(|e| Error::CorruptSnapshot(format!("invalid snapshot lsn '{stem}': {e}")))
}
