//! Manages the set of volume files: active writer rotation and reader cache.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;

use crate::format::RecordHeader;
use crate::volume::{RecordLocation, VolumeReader, VolumeWriter};
use crate::{BlobStoreOptions, Result};

const VOLUME_FILENAME_WIDTH: usize = 12;

/// Manages volume writers and a cache of readers.
#[derive(Debug)]
pub struct VolumeManager {
    volumes_dir: PathBuf,
    max_volume_size: u64,
    next_volume_number: AtomicU64,
    active_writer: Mutex<Option<VolumeWriter>>,
    readers: DashMap<u64, Arc<VolumeReader>>,
}

impl VolumeManager {
    /// Open or create the volume manager.
    pub fn open(path: impl AsRef<Path>, options: &BlobStoreOptions) -> Result<Self> {
        let volumes_dir = path.as_ref().join("volumes");
        fs::create_dir_all(&volumes_dir)?;
        let max_number = list_existing_volume_numbers(&volumes_dir)?;
        Ok(Self {
            volumes_dir,
            max_volume_size: options.max_volume_size,
            next_volume_number: AtomicU64::new(max_number + 1),
            active_writer: Mutex::new(None),
            readers: DashMap::new(),
        })
    }

    /// Append a record for `id` with payload from `reader`.
    ///
    /// Returns the record location.  This serializes appends internally.
    pub fn append_record(
        &self,
        id: &[u8],
        reader: &mut dyn std::io::Read,
    ) -> Result<(RecordLocation, RecordHeader)> {
        let mut guard = self.active_writer.lock().unwrap();

        // Rotate if the active volume has reached its soft size limit and is not empty.
        let should_rotate = match guard.as_ref() {
            Some(w) => w.size() >= self.max_volume_size && w.size() > 0,
            None => true,
        };

        if should_rotate {
            let number = self.next_volume_number.fetch_add(1, Ordering::SeqCst);
            let path = volume_path(&self.volumes_dir, number);
            *guard = Some(VolumeWriter::create(path, number)?);
        }

        let writer = guard.as_mut().unwrap();
        let (location, header) = writer.append_record(id, reader)?;
        Ok((location, header))
    }

    /// Return a shared reader for `volume_number`, opening it if necessary.
    pub fn reader(&self, volume_number: u64) -> Result<Arc<VolumeReader>> {
        if let Some(entry) = self.readers.get(&volume_number) {
            return Ok(Arc::clone(entry.value()));
        }
        let path = volume_path(&self.volumes_dir, volume_number);
        let reader = Arc::new(VolumeReader::open(path, volume_number)?);
        self.readers.insert(volume_number, Arc::clone(&reader));
        Ok(reader)
    }

    /// Sync the active volume file.
    pub fn sync(&self) -> Result<()> {
        if let Some(writer) = self.active_writer.lock().unwrap().as_ref() {
            writer.sync()?;
        }
        Ok(())
    }

    /// Truncate the active volume to `size` bytes.
    pub fn truncate_active_volume(&self, size: u64) -> Result<()> {
        if let Some(writer) = self.active_writer.lock().unwrap().as_mut() {
            writer.truncate(size)?;
        }
        Ok(())
    }

    /// Remove a volume reader from the cache so the file can be deleted.
    pub fn evict_reader(&self, volume_number: u64) {
        self.readers.remove(&volume_number);
    }

    /// Path to a volume file.
    pub fn volume_path(&self, number: u64) -> PathBuf {
        volume_path(&self.volumes_dir, number)
    }

    /// Directory containing volume files.
    pub fn volumes_dir(&self) -> &Path {
        &self.volumes_dir
    }

    /// Current active volume number, if any.
    pub fn active_volume_number(&self) -> Option<u64> {
        self.active_writer.lock().unwrap().as_ref().map(|w| w.number())
    }
}

fn volume_path(volumes_dir: &Path, number: u64) -> PathBuf {
    volumes_dir.join(format!("{:0width$}.blob", number, width = VOLUME_FILENAME_WIDTH))
}

fn list_existing_volume_numbers(volumes_dir: &Path) -> Result<u64> {
    let mut max = 0u64;
    if !volumes_dir.exists() {
        return Ok(max);
    }
    for entry in fs::read_dir(volumes_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".blob")
            && let Ok(n) = stem.parse::<u64>()
        {
            max = max.max(n);
        }
    }
    Ok(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    fn mgr(dir: &TempDir) -> VolumeManager {
        VolumeManager::open(dir.path(), &BlobStoreOptions::default()).unwrap()
    }

    #[test]
    fn append_and_reader_cache() {
        let dir = TempDir::new().unwrap();
        let mgr = mgr(&dir);
        let (loc, _header) = mgr.append_record(b"a", &mut Cursor::new(&b"data"[..])).unwrap();
        assert_eq!(loc.volume_number, 1);

        let reader = mgr.reader(loc.volume_number).unwrap();
        let (_h, id, payload) = reader.read_record(loc.offset).unwrap();
        assert_eq!(id.as_ref(), b"a");
        assert_eq!(payload.as_ref(), b"data");
    }
}
