//! Checkpoint and backup support.
//!
//! A checkpoint is a consistent, read-only, point-in-time snapshot of the
//! database created cheaply with hard links.  A backup is a named checkpoint
//! stored under `<engine>/backups/<name>`.
//!
//! # Correctness
//!
//! * The engine is synced before the snapshot is taken, so every write that was
//!   visible at the call site is flushed to SSTables.
//! * The current [`Version`] of every column family is pinned while files are
//!   copied/hard-linked, so concurrent compactions cannot delete SSTables that
//!   the checkpoint needs.
//! * The checkpoint receives a *fresh* manifest that records exactly the pinned
//!   versions.  This guarantees the checkpoint is a true point-in-time snapshot
//!   rather than a copy of the live manifest that may contain later edits.
//! * SSTable files are immutable, so hard links are safe.  The manifest is
//!   always copied (never hard-linked) because it must be frozen at the
//!   checkpoint sequence.
//!
//! # Limitations
//!
//! * Restore copies a backup to a new directory; it does not overwrite the
//!   running engine's files.  The restored directory can be opened as a normal
//!   engine.
//! * Backups are local to the same filesystem as the engine for hard-link
//!   efficiency; when hard links are unavailable the implementation falls back
//!   to full file copies.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::LsmEngine;
use crate::immutable::sstable_path;
use crate::manifest::Manifest;
use crate::version::FileMetaData;
use crate::version_set::VersionEdit;
use crate::{Error, FileNumber, Result};

const BACKUP_MANIFEST_FILE: &str = "BACKUP";
const BACKUPS_SUBDIR: &str = "backups";
const MANIFEST_NAME: &str = "MANIFEST-000001";
const CURRENT_NAME: &str = "CURRENT";

/// On-disk description of a single backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub version: u32,
    pub sequence: u64,
    /// Unix timestamp (seconds since epoch) when the backup was created.
    pub created_at: u64,
    pub column_families: Vec<BackupColumnFamily>,
}

impl BackupManifest {
    fn new(sequence: u64, column_families: Vec<BackupColumnFamily>) -> Self {
        Self {
            version: 1,
            sequence,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            column_families,
        }
    }
}

/// Per-column-family metadata stored in a backup manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupColumnFamily {
    pub id: u32,
    pub name: String,
    pub files: Vec<FileNumber>,
}

/// Create a checkpoint of `engine` in `dir`.
///
/// `dir` must not exist or must be empty.  The checkpoint is a self-contained
/// engine directory that can be opened with [`LsmEngine::open`].
pub fn create_checkpoint(engine: &LsmEngine, dir: impl AsRef<Path>) -> Result<()> {
    let dir = dir.as_ref();
    if dir.exists() {
        let mut entries = fs::read_dir(dir)?;
        if entries.next().is_some() {
            return Err(Error::InvalidArgument(format!(
                "checkpoint target {} is not empty",
                dir.display()
            )));
        }
    } else {
        fs::create_dir_all(dir)?;
    }

    // Flush everything, then force-freeze every active MemTable so that the
    // checkpoint contains only SSTables and no mutable state.
    engine.inner().sync()?;
    {
        let state = engine.inner().state.lock().unwrap();
        let cf_ids: Vec<_> = state.column_families.iter().map(|cf| cf.id).collect();
        drop(state);
        for cf_id in cf_ids {
            engine.inner().force_freeze(cf_id)?;
        }
    }
    engine.inner().sync()?;

    // Pin the current version of every column family under the engine lock.
    let (path, sequence, pinned) = {
        let state = engine.inner().state.lock().unwrap();
        let path = state.path.clone();
        let sequence = state.seq_allocator.completed();
        let mut pinned = Vec::with_capacity(state.column_families.len());
        for cf in state.column_families.iter() {
            let version = cf.version_set.current();
            let files: Vec<FileMetaData> = version
                .levels
                .iter()
                .flat_map(|level| level.iter())
                .cloned()
                .collect();
            pinned.push((cf.id, cf.name.clone(), cf.options.clone(), version, files));
        }
        (path, sequence, pinned)
    };

    // Build the checkpoint manifest from the pinned state.
    let manifest_path = dir.join(MANIFEST_NAME);
    let mut manifest = Manifest::create(&manifest_path)?;
    let mut next_file_number: FileNumber = 1;
    let mut backup_cfs = Vec::with_capacity(pinned.len());

    for (cf_id, name, _options, version, files) in &pinned {
        // Record CF creation first.
        manifest.log_edit(&VersionEdit {
            cf_id: *cf_id,
            created_cfs: vec![(*cf_id, name.clone())],
            next_file_number,
            last_sequence: sequence,
            ..Default::default()
        })?;

        // Add every file from every level.
        for (level, level_files) in version.levels.iter().enumerate() {
            if level_files.is_empty() {
                continue;
            }
            let new_files: Vec<(usize, FileMetaData)> = level_files
                .iter()
                .map(|m| {
                    next_file_number = next_file_number.max(m.number + 1);
                    (level, m.clone())
                })
                .collect();
            manifest.log_edit(&VersionEdit {
                cf_id: *cf_id,
                new_files,
                next_file_number,
                last_sequence: sequence,
                ..Default::default()
            })?;
        }

        backup_cfs.push(BackupColumnFamily {
            id: *cf_id,
            name: name.clone(),
            files: files.iter().map(|m| m.number).collect(),
        });
    }

    // Final edit carries the authoritative next_file_number/last_sequence.
    if let Some((cf_id, _, _, _, _)) = pinned.first() {
        manifest.log_edit(&VersionEdit {
            cf_id: *cf_id,
            next_file_number,
            last_sequence: sequence,
            ..Default::default()
        })?;
    }

    manifest.sync()?;

    // Write CURRENT.
    let mut current = fs::File::create(dir.join(CURRENT_NAME))?;
    current.write_all(format!("{MANIFEST_NAME}\n").as_bytes())?;
    current.sync_all()?;

    // Copy/hard-link the SSTable files referenced by the pinned versions.
    let mut linked = HashSet::new();
    for (_, _, _, _, files) in &pinned {
        for meta in files {
            if !linked.insert(meta.number) {
                continue;
            }
            let src = sstable_path(&path, meta.number);
            let dst = sstable_path(dir, meta.number);
            link_or_copy_file(&src, &dst)?;
        }
    }

    // Write the human-readable backup manifest.
    let backup_manifest = BackupManifest::new(sequence, backup_cfs);
    let json = serde_json::to_vec_pretty(&backup_manifest)?;
    let mut backup_file = fs::File::create(dir.join(BACKUP_MANIFEST_FILE))?;
    backup_file.write_all(&json)?;
    backup_file.sync_all()?;

    // Sync the checkpoint directory itself so the new entries are durable.
    sync_dir(dir)?;

    Ok(())
}

/// Create a named backup under `<engine>/backups/<name>`.
pub fn create_backup(engine: &LsmEngine, name: &str) -> Result<()> {
    validate_backup_name(name)?;
    let dir = backup_dir(engine, name)?;
    if dir.exists() {
        return Err(Error::InvalidArgument(format!(
            "backup '{}' already exists",
            name
        )));
    }
    create_checkpoint(engine, &dir)
}

/// Restore a named backup to `target`.
///
/// `target` must not exist or must be an empty directory.  The restored
/// directory is a self-contained engine and can be opened with
/// [`LsmEngine::open`].
pub fn restore_backup(engine: &LsmEngine, name: &str, target: impl AsRef<Path>) -> Result<()> {
    let source = backup_dir(engine, name)?;
    if !source.exists() {
        return Err(Error::InvalidArgument(format!("backup '{}' not found", name)));
    }
    restore_checkpoint(&source, target)
}

/// Restore an arbitrary checkpoint directory to `target`.
pub fn restore_checkpoint(source: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
    let source = source.as_ref();
    let target = target.as_ref();

    if !source.join(BACKUP_MANIFEST_FILE).exists() {
        return Err(Error::InvalidArgument(format!(
            "{} is not a valid checkpoint (missing {})",
            source.display(),
            BACKUP_MANIFEST_FILE
        )));
    }

    if target.exists() {
        let mut entries = fs::read_dir(target)?;
        if entries.next().is_some() {
            return Err(Error::InvalidArgument(format!(
                "restore target {} is not empty",
                target.display()
            )));
        }
    } else {
        fs::create_dir_all(target)?;
    }

    // Copy every file except BACKUP (which is only human-readable metadata).
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if file_name == BACKUP_MANIFEST_FILE {
            continue;
        }
        let src = entry.path();
        let dst = target.join(&file_name);
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }

    sync_dir(target)?;
    Ok(())
}

/// Delete a named backup.
pub fn delete_backup(engine: &LsmEngine, name: &str) -> Result<()> {
    let dir = backup_dir(engine, name)?;
    if !dir.exists() {
        return Err(Error::InvalidArgument(format!("backup '{}' not found", name)));
    }
    fs::remove_dir_all(&dir)?;
    Ok(())
}

/// List the names of all backups under the engine.
pub fn list_backups(engine: &LsmEngine) -> Result<Vec<String>> {
    let backups_root = {
        let state = engine.inner().state.lock().unwrap();
        state.path.join(BACKUPS_SUBDIR)
    };
    let mut names = Vec::new();
    if backups_root.exists() {
        for entry in fs::read_dir(&backups_root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    names.sort();
    Ok(names)
}

fn validate_backup_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidArgument("backup name cannot be empty".into()));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(Error::InvalidArgument(
            "backup name cannot contain path separators".into(),
        ));
    }
    Ok(())
}

fn backup_dir(engine: &LsmEngine, name: &str) -> Result<PathBuf> {
    let state = engine.inner().state.lock().unwrap();
    Ok(state.path.join(BACKUPS_SUBDIR).join(name))
}

/// Try to hard-link `src` to `dst`; fall back to a full copy if linking fails.
fn link_or_copy_file(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::hard_link(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(src, dst)?;
            Ok(())
        }
    }
}

fn copy_dir_recursive(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Best-effort directory sync.
fn sync_dir(dir: impl AsRef<Path>) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(dir.as_ref())?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LsmOptions;

    #[test]
    fn checkpoint_empty_engine() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint = tempfile::tempdir().unwrap();
        let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();
        create_checkpoint(&engine, checkpoint.path()).unwrap();

        let backup_path = checkpoint.path().join(BACKUP_MANIFEST_FILE);
        assert!(backup_path.exists());
        let manifest: BackupManifest =
            serde_json::from_slice(&fs::read(&backup_path).unwrap()).unwrap();
        assert_eq!(manifest.column_families.len(), 1);
        assert!(manifest.column_families[0].files.is_empty());
    }

    #[test]
    fn checkpoint_reopens_consistently() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint = tempfile::tempdir().unwrap();
        let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();
        engine.put(b"a", b"1").unwrap();
        engine.put(b"b", b"2").unwrap();
        engine.sync().unwrap();
        create_checkpoint(&engine, checkpoint.path()).unwrap();

        let reopened = LsmEngine::open(checkpoint.path(), LsmOptions::default()).unwrap();
        assert_eq!(reopened.get(b"a").unwrap().unwrap().as_ref(), b"1");
        assert_eq!(reopened.get(b"b").unwrap().unwrap().as_ref(), b"2");
    }

    #[test]
    fn backup_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let restore = tempfile::tempdir().unwrap();
        let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();
        engine.put(b"x", b"y").unwrap();
        engine.sync().unwrap();
        create_backup(&engine, "snap1").unwrap();

        let names = list_backups(&engine).unwrap();
        assert_eq!(names, vec!["snap1"]);

        restore_backup(&engine, "snap1", restore.path()).unwrap();
        let reopened = LsmEngine::open(restore.path(), LsmOptions::default()).unwrap();
        assert_eq!(reopened.get(b"x").unwrap().unwrap().as_ref(), b"y");
    }

    #[test]
    fn delete_backup_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();
        create_backup(&engine, "to-delete").unwrap();
        delete_backup(&engine, "to-delete").unwrap();
        assert!(list_backups(&engine).unwrap().is_empty());
    }
}
