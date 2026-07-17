//! Manifest snapshot write/load/truncate helpers.
//!
//! A snapshot is a JSON-serialized `Manifest` plus its LSN. The `CURRENT` file
//! points to the latest snapshot name. After a snapshot is written and `CURRENT`
//! is updated, old WAL segments can be truncated.

use std::path::{Path, PathBuf};

use crate::manifest::Manifest;
use crate::{Error, Result};

const CURRENT_FILE: &str = "CURRENT";
const SNAPSHOT_DIR: &str = "manifest-snapshot";

/// Write a snapshot of `manifest` at `lsn` and update the `CURRENT` pointer.
///
/// The snapshot is written to a temp file and atomically renamed so that a
/// crash can never leave `CURRENT` pointing at a partially-written snapshot.
pub fn write(path: &Path, manifest: &Manifest, lsn: u64) -> Result<PathBuf> {
    let snapshot_dir = path.join(SNAPSHOT_DIR);
    std::fs::create_dir_all(&snapshot_dir)?;

    let snapshot_name = format!("{:020}.snapshot", lsn);
    let snapshot_path = snapshot_dir.join(&snapshot_name);
    let temp_path = snapshot_dir.join(format!(".{}.tmp", snapshot_name));

    let json = serde_json::to_vec(manifest)?;
    std::fs::write(&temp_path, json)?;

    let temp_file = std::fs::File::open(&temp_path)?;
    temp_file.sync_all()?;
    drop(temp_file);

    std::fs::rename(&temp_path, &snapshot_path)?;
    let snapshot_dir_file = std::fs::File::open(&snapshot_dir)?;
    snapshot_dir_file.sync_all()?;
    drop(snapshot_dir_file);

    let current_path = path.join(CURRENT_FILE);
    std::fs::write(&current_path, &snapshot_name)?;
    let current_file = std::fs::File::open(&current_path)?;
    current_file.sync_all()?;
    drop(current_file);
    let table_dir = std::fs::File::open(path)?;
    table_dir.sync_all()?;
    drop(table_dir);

    Ok(snapshot_path)
}

/// Load the latest snapshot and its LSN, if one exists.
pub fn load(path: &Path) -> Result<(Manifest, u64)> {
    let current_path = path.join(CURRENT_FILE);
    let current = std::fs::read_to_string(&current_path)?;
    let snapshot_name = current.trim();
    let snapshot_path = path.join(SNAPSHOT_DIR).join(snapshot_name);
    let bytes = std::fs::read(&snapshot_path)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| Error::ManifestWal(format!("failed to decode snapshot: {e}")))?;

    let lsn = parse_lsn(snapshot_name)?;
    Ok((manifest, lsn))
}

fn parse_lsn(name: &str) -> Result<u64> {
    let stem = name.strip_suffix(".snapshot").ok_or_else(|| {
        Error::ManifestWal(format!("snapshot name missing .snapshot suffix: {name}"))
    })?;
    stem.parse::<u64>()
        .map_err(|e| Error::ManifestWal(format!("invalid snapshot lsn '{stem}': {e}")))
}
