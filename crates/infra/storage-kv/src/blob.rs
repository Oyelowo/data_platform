//! WiscKey-style value separation.
//!
//! Large values are written to an append-only value log and a small `BlobRef` is
//! stored in the LSM tree instead of the value itself.  This dramatically
//! reduces write amplification for large values because compactions only move
//! the 24-byte references, not the payloads.
//!
//! # Value log format
//!
//! ```text
//! | value_len (LE64) | cf_id (LE32) | key_len (LE32) | value_crc (LE32) | seq (LE64) | reserved (LE32) | key bytes | value bytes | padding to 8 |
//! ```
//!
//! The 32-byte header stores the value length, column family, key length, a
//! CRC32C over the concatenation `key || value`, and the original LSM sequence
//! number of the referencing entry.  The sequence number is preserved so that
//! online garbage collection can rewrite a blob reference with the same
//! internal key, guaranteeing that a concurrent foreground write with a higher
//! sequence number cannot be shadowed by the GC rewrite.  Records are 8-byte
//! aligned so that positioned reads stay aligned.  The key is stored alongside
//! the value so that online garbage collection can look up the owning key in
//! the LSM without scanning it.
//!
//! # BlobRef
//!
//! A `BlobRef` is 24 bytes: `(file_number, offset, len)` each as little-endian
//! `u64`.  `len` is the value length.  It is stored as the "value" of an LSM
//! entry whose internal-key type is
//! [`ValueType::BlobRef`](crate::internal_key::ValueType::BlobRef).
//!
//! # Concurrency
//!
//! Appends to the current blob file are serialized by a mutex.  Reads and
//! garbage-collection scans are lock-free positioned reads on immutable files.
//! This matches the expected workload: large-value writes are relatively rare,
//! and point reads of those values dominate.
//!
//! Active reads are reference-counted per blob file so that garbage collection
//! never deletes a file while an in-flight reader may still dereference a
//! `BlobRef` that points into it.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::{Buf, BufMut, Bytes};

use crate::Result;
use crate::column_family::ColumnFamilyId;
use crate::sstable::format::checksum;
use crate::{Error, FileNumber, SequenceNumber};

const BLOB_HEADER_SIZE: u64 = 32;
const BLOB_DIR: &str = "blob";

/// Reference to a value stored in a blob file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobRef {
    pub file_number: FileNumber,
    pub offset: u64,
    pub len: u64,
}

impl BlobRef {
    /// Encode to a 24-byte little-endian representation.
    pub fn encode(&self) -> [u8; 24] {
        let mut buf = [0u8; 24];
        {
            let mut cursor = &mut buf[..];
            cursor.put_u64_le(self.file_number);
            cursor.put_u64_le(self.offset);
            cursor.put_u64_le(self.len);
        }
        buf
    }

    /// Decode a 24-byte little-endian blob reference.
    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() != 24 {
            return None;
        }
        let mut cursor = buf;
        Some(Self {
            file_number: cursor.get_u64_le(),
            offset: cursor.get_u64_le(),
            len: cursor.get_u64_le(),
        })
    }
}

/// Header stored before each key/value pair in a blob file.
///
/// Layout (32 bytes, little-endian):
///
/// ```text
/// | value_len (u64) | cf_id (u32) | key_len (u32) | value_crc (u32) | seq (u64) | reserved (u32) |
/// ```
///
/// `value_crc` covers the concatenation `key || value`.  `reserved` is zeroed
/// and available for future format extensions.  `seq` is the LSM sequence
/// number of the entry that references this blob.
#[derive(Debug, Clone, Copy)]
struct BlobRecordHeader {
    value_len: u64,
    cf_id: u32,
    key_len: u32,
    crc: u32,
    seq: SequenceNumber,
}

impl BlobRecordHeader {
    fn encode(&self) -> [u8; BLOB_HEADER_SIZE as usize] {
        let mut buf = [0u8; BLOB_HEADER_SIZE as usize];
        {
            let mut cursor = &mut buf[..];
            cursor.put_u64_le(self.value_len);
            cursor.put_u32_le(self.cf_id);
            cursor.put_u32_le(self.key_len);
            cursor.put_u32_le(self.crc);
            cursor.put_u64_le(self.seq);
            // remaining 4 bytes are left zeroed (reserved)
        }
        buf
    }

    fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < 28 {
            return None;
        }
        let mut cursor = buf;
        Some(Self {
            value_len: cursor.get_u64_le(),
            cf_id: cursor.get_u32_le(),
            key_len: cursor.get_u32_le(),
            crc: cursor.get_u32_le(),
            seq: cursor.get_u64_le(),
        })
    }

    /// Total padded size of the record on disk.
    fn padded_size(&self) -> u64 {
        let raw = BLOB_HEADER_SIZE + self.key_len as u64 + self.value_len as u64;
        align_up(raw, 8)
    }
}

/// A record read from a blob file, used by garbage collection.
#[derive(Debug, Clone)]
pub struct BlobRecord {
    #[allow(dead_code)]
    pub file_number: FileNumber,
    pub offset: u64,
    pub cf_id: ColumnFamilyId,
    pub seq: SequenceNumber,
    pub key: Bytes,
    pub value: Bytes,
}

/// Append-only writer for the current blob file.
struct BlobLogWriter {
    file: File,
    file_number: FileNumber,
    offset: u64,
}

impl BlobLogWriter {
    fn open(path: &Path, file_number: FileNumber) -> Result<Self> {
        let file_path = blob_file_path(path, file_number);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&file_path)?;
        let offset = file.metadata()?.len();
        Ok(Self {
            file,
            file_number,
            offset,
        })
    }

    /// Append `key`/`value` for column family `cf_id` at LSM sequence `seq` and
    /// return a reference to the value.  The record is aligned to 8 bytes so
    /// future positioned reads start on aligned boundaries.
    fn append(
        &mut self,
        cf_id: ColumnFamilyId,
        key: &[u8],
        value: &[u8],
        seq: SequenceNumber,
    ) -> Result<BlobRef> {
        let offset = self.offset;
        let combined = [key, value].concat();
        let header = BlobRecordHeader {
            value_len: value.len() as u64,
            cf_id,
            key_len: key.len() as u32,
            crc: checksum(&combined),
            seq,
        };
        self.file.write_all(&header.encode())?;
        self.file.write_all(key)?;
        self.file.write_all(value)?;

        let padded_size = header.padded_size();
        let raw_size = BLOB_HEADER_SIZE + key.len() as u64 + value.len() as u64;
        let padding = padded_size - raw_size;
        if padding > 0 {
            self.file.write_all(&vec![0u8; padding as usize])?;
        }
        self.file.flush()?;
        self.file.sync_all()?;
        self.offset += padded_size;

        Ok(BlobRef {
            file_number: self.file_number,
            offset,
            len: value.len() as u64,
        })
    }
}

/// RAII guard that reference-counts an active read against a blob file.
struct BlobReadLease {
    file_number: FileNumber,
    registry: Arc<Mutex<HashMap<FileNumber, AtomicUsize>>>,
}

impl Drop for BlobReadLease {
    fn drop(&mut self) {
        let mut reg = self.registry.lock().unwrap();
        if let Some(counter) = reg.get_mut(&self.file_number) {
            // `fetch_sub` returns the previous value; if it was 1 the count is
            // now 0 and the entry can be removed.
            if counter.fetch_sub(1, Ordering::Release) == 1 {
                reg.remove(&self.file_number);
            }
        }
    }
}

/// Manages the append-only value log.
pub struct BlobStore {
    path: PathBuf,
    writer: Mutex<BlobLogWriter>,
    file_size_threshold: u64,
    /// Per-file reference counts for in-flight readers.  A file is considered
    /// idle for deletion when it has no entry here.
    active_reads: Arc<Mutex<HashMap<FileNumber, AtomicUsize>>>,
    /// Blob files that have been rewritten but still had active readers at the
    /// time.  They are retried on future GC passes.
    pending_deletes: Mutex<HashSet<FileNumber>>,
}

impl BlobStore {
    /// Open or create a blob store under `db_path/blob`.
    pub fn open(db_path: impl AsRef<Path>, file_size_threshold: u64) -> Result<Self> {
        let path = db_path.as_ref().join(BLOB_DIR);
        std::fs::create_dir_all(&path)?;
        let next_number = next_blob_file_number(&path);
        let writer = BlobLogWriter::open(&path, next_number)?;
        Ok(Self {
            path,
            writer: Mutex::new(writer),
            file_size_threshold,
            active_reads: Arc::new(Mutex::new(HashMap::new())),
            pending_deletes: Mutex::new(HashSet::new()),
        })
    }

    /// Store `key`/`value` for column family `cf_id` at LSM sequence `seq` in
    /// the blob log and return a reference to the value.
    pub fn put(
        &self,
        cf_id: ColumnFamilyId,
        key: &[u8],
        value: &[u8],
        seq: SequenceNumber,
    ) -> Result<BlobRef> {
        let mut writer = self.writer.lock().unwrap();
        if writer.offset >= self.file_size_threshold && !value.is_empty() {
            let next_number = writer.file_number + 1;
            *writer = BlobLogWriter::open(&self.path, next_number)?;
        }
        writer.append(cf_id, key, value, seq)
    }

    fn acquire_lease(&self, file_number: FileNumber) -> BlobReadLease {
        let mut reg = self.active_reads.lock().unwrap();
        let counter = reg.entry(file_number).or_insert_with(AtomicUsize::default);
        counter.fetch_add(1, Ordering::Acquire);
        BlobReadLease {
            file_number,
            registry: Arc::clone(&self.active_reads),
        }
    }

    /// True when no reader currently holds a lease on `file_number`.
    fn is_idle(&self, file_number: FileNumber) -> bool {
        let reg = self.active_reads.lock().unwrap();
        !reg.contains_key(&file_number)
    }

    /// Read the value referenced by `blob_ref`.
    pub fn get(&self, blob_ref: BlobRef) -> Result<Bytes> {
        let (_, value) = self.read_record(blob_ref)?;
        Ok(value)
    }

    /// Read the key and value referenced by `blob_ref`.
    #[allow(dead_code)]
    pub fn get_key_value(&self, blob_ref: BlobRef) -> Result<(Bytes, Bytes)> {
        self.read_record(blob_ref)
    }

    fn read_record(&self, blob_ref: BlobRef) -> Result<(Bytes, Bytes)> {
        let path = blob_file_path(&self.path, blob_ref.file_number);
        let file = File::open(&path)?;
        let _lease = self.acquire_lease(blob_ref.file_number);
        let mut header = [0u8; BLOB_HEADER_SIZE as usize];
        file.read_exact_at(&mut header, blob_ref.offset)?;
        let header = BlobRecordHeader::decode(&header)
            .ok_or_else(|| Error::Blob("bad blob record header".into()))?;
        if header.value_len != blob_ref.len {
            return Err(Error::Blob(format!(
                "blob length mismatch: ref {} != header {}",
                blob_ref.len, header.value_len
            )));
        }

        let key_offset = blob_ref.offset + BLOB_HEADER_SIZE;
        let value_offset = key_offset + header.key_len as u64;
        let mut key = vec![0u8; header.key_len as usize];
        file.read_exact_at(&mut key, key_offset)?;
        let mut value = vec![0u8; header.value_len as usize];
        file.read_exact_at(&mut value, value_offset)?;

        let combined = [key.as_slice(), value.as_slice()].concat();
        if checksum(&combined) != header.crc {
            return Err(Error::Blob("blob checksum mismatch".into()));
        }
        Ok((Bytes::from(key), Bytes::from(value)))
    }

    /// Iterate over all records in `file_number` in file order.
    ///
    /// This is used by the garbage collector.  The file is read with positioned
    /// reads and is not locked against concurrent appends to other files.
    pub fn iter_file(&self, file_number: FileNumber) -> Result<BlobFileIterator> {
        let path = blob_file_path(&self.path, file_number);
        let file = File::open(&path)?;
        let file_len = file.metadata()?.len();
        Ok(BlobFileIterator {
            file,
            file_number,
            file_len,
            offset: 0,
        })
    }

    /// Return the path to the blob directory.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the file number currently being written.
    pub fn current_file_number(&self) -> FileNumber {
        self.writer.lock().unwrap().file_number
    }

    /// List all blob file numbers on disk, sorted ascending.
    pub fn list_files(&self) -> Result<Vec<FileNumber>> {
        let mut numbers = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(stem) = name.strip_suffix(".blob") {
                    if let Ok(n) = stem.parse::<u64>() {
                        numbers.push(n);
                    }
                }
            }
        }
        numbers.sort_unstable();
        Ok(numbers)
    }

    /// Return the size of `file_number` in bytes, or `None` if it does not exist.
    pub fn file_size(&self, file_number: FileNumber) -> Result<Option<u64>> {
        let path = blob_file_path(&self.path, file_number);
        match std::fs::metadata(&path) {
            Ok(meta) => Ok(Some(meta.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete `file_number` from disk regardless of active readers.  Callers
    /// must ensure no live reference points into this file.
    #[allow(dead_code)]
    pub fn delete_file(&self, file_number: FileNumber) -> Result<()> {
        let path = blob_file_path(&self.path, file_number);
        std::fs::remove_file(&path)?;
        Ok(())
    }

    /// Try to delete `file_number`.  Returns `Ok(true)` if the file was deleted
    /// (or did not exist), and `Ok(false)` if active readers prevent deletion.
    pub fn try_delete_file(&self, file_number: FileNumber) -> Result<bool> {
        if !self.is_idle(file_number) {
            return Ok(false);
        }
        let path = blob_file_path(&self.path, file_number);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(e.into()),
        }
    }

    fn add_pending_delete(&self, file_number: FileNumber) {
        self.pending_deletes.lock().unwrap().insert(file_number);
    }

    /// Attempt to delete all pending blob files that are now idle.  Returns the
    /// number of files deleted.
    fn drain_pending_deletes(&self) -> Result<u64> {
        let mut pending = self.pending_deletes.lock().unwrap();
        let files: Vec<FileNumber> = pending.iter().copied().collect();
        let mut deleted = 0u64;
        for file_number in files {
            match self.try_delete_file(file_number) {
                Ok(true) => {
                    pending.remove(&file_number);
                    deleted += 1;
                }
                Ok(false) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(deleted)
    }
}

/// Owner of the LSM tree that garbage collection consults to decide whether a
/// blob record is still live and to durably rewrite live records.
pub trait BlobOwner {
    /// Return `true` if `blob_ref` is the newest visible blob reference for
    /// `(cf_id, key)` at `snapshot`.
    fn is_blob_live(
        &self,
        cf_id: ColumnFamilyId,
        key: &[u8],
        blob_ref: BlobRef,
        snapshot: SequenceNumber,
    ) -> bool;

    /// Rewrite `value` for `(cf_id, key)` at the original LSM sequence `seq`.
    ///
    /// Implementations may buffer the rewrite and flush it durably later; see
    /// [`Self::commit`].
    fn rewrite_blob(
        &mut self,
        cf_id: ColumnFamilyId,
        key: &[u8],
        value: &[u8],
        seq: SequenceNumber,
    ) -> Result<()>;

    /// Flush any buffered rewrites so that the new blob references are durable
    /// before the old blob file is deleted.
    fn commit(&mut self) -> Result<()>;
}

/// Options controlling blob garbage collection.
#[derive(Debug, Clone, Copy)]
pub struct GcOptions {
    /// Minimum ratio of live bytes to total bytes that keeps a blob file alive.
    /// Files with a live ratio strictly below this threshold are rewritten.
    pub min_live_ratio: f64,
}

/// Statistics returned by a single GC pass.
#[derive(Debug, Default, Clone, Copy)]
pub struct GcStats {
    pub scanned_files: u64,
    pub rewritten_records: u64,
    pub rewritten_bytes: u64,
    pub dead_records: u64,
    pub dead_bytes: u64,
    pub deleted_files: u64,
    pub space_reclaimed: u64,
}

impl BlobStore {
    /// Run one pass of blob garbage collection.
    ///
    /// For each non-current blob file whose live ratio is below
    /// `options.min_live_ratio`, live records are rewritten through `owner` and
    /// the old file is deleted once the rewrites are committed and no active
    /// reader references the file.
    ///
    /// `snapshot` must be the oldest sequence number that may still be observed
    /// by an in-flight reader.  Records visible at or after this snapshot are
    /// considered live.
    pub fn gc_once(
        &self,
        owner: &mut dyn BlobOwner,
        options: &GcOptions,
        snapshot: SequenceNumber,
    ) -> Result<GcStats> {
        let mut stats = GcStats::default();

        // Finish cleaning up files that were deferred because of active readers.
        stats.deleted_files += self.drain_pending_deletes()?;

        let current = self.current_file_number();
        let files = self.list_files()?;

        for file_number in files {
            if file_number == current {
                continue;
            }
            let Some(total_bytes) = self.file_size(file_number)? else {
                continue;
            };
            if total_bytes == 0 {
                let _ = self.try_delete_file(file_number);
                continue;
            }

            stats.scanned_files += 1;

            // First pass: classify records and compute the live ratio.
            let mut iter = self.iter_file(file_number)?;
            let mut records: Vec<BlobRecord> = Vec::new();
            let mut live_bytes: u64 = 0;
            while let Some(rec) = iter.next_record()? {
                let rec_size =
                    align_up(BLOB_HEADER_SIZE + rec.key.len() as u64 + rec.value.len() as u64, 8);
                let blob_ref = BlobRef {
                    file_number,
                    offset: rec.offset,
                    len: rec.value.len() as u64,
                };
                if owner.is_blob_live(rec.cf_id, &rec.key, blob_ref, snapshot) {
                    live_bytes += rec_size;
                }
                records.push(rec);
            }

            let live_ratio = live_bytes as f64 / total_bytes as f64;
            if live_ratio >= options.min_live_ratio {
                continue;
            }

            // Second pass: rewrite live records and drop dead ones.
            for rec in records {
                let blob_ref = BlobRef {
                    file_number,
                    offset: rec.offset,
                    len: rec.value.len() as u64,
                };
                if owner.is_blob_live(rec.cf_id, &rec.key, blob_ref, snapshot) {
                    owner.rewrite_blob(rec.cf_id, &rec.key, &rec.value, rec.seq)?;
                    stats.rewritten_records += 1;
                    stats.rewritten_bytes += rec.value.len() as u64;
                } else {
                    stats.dead_records += 1;
                    stats.dead_bytes += rec.value.len() as u64;
                }
            }
            owner.commit()?;

            let dead_bytes = total_bytes - live_bytes;
            if self.try_delete_file(file_number)? {
                stats.deleted_files += 1;
                stats.space_reclaimed += dead_bytes;
            } else {
                self.add_pending_delete(file_number);
            }
        }

        Ok(stats)
    }
}

/// Iterator over records in a single blob file.
pub struct BlobFileIterator {
    file: File,
    file_number: FileNumber,
    file_len: u64,
    offset: u64,
}

impl BlobFileIterator {
    /// Return the next record, or `None` when the file is exhausted.
    pub fn next_record(&mut self) -> Result<Option<BlobRecord>> {
        if self.offset >= self.file_len {
            return Ok(None);
        }
        let mut header = [0u8; BLOB_HEADER_SIZE as usize];
        self.file.read_exact_at(&mut header, self.offset)?;
        let header = BlobRecordHeader::decode(&header)
            .ok_or_else(|| Error::Blob("bad blob record header during GC scan".into()))?;

        let key_offset = self.offset + BLOB_HEADER_SIZE;
        let value_offset = key_offset + header.key_len as u64;
        let record_end = value_offset + header.value_len as u64;
        let padded_end = self.offset + header.padded_size();
        if record_end > self.file_len {
            return Err(Error::Blob("blob record extends past file end".into()));
        }

        let mut key = vec![0u8; header.key_len as usize];
        self.file.read_exact_at(&mut key, key_offset)?;
        let mut value = vec![0u8; header.value_len as usize];
        self.file.read_exact_at(&mut value, value_offset)?;

        let combined = [key.as_slice(), value.as_slice()].concat();
        if checksum(&combined) != header.crc {
            return Err(Error::Blob("blob checksum mismatch during GC scan".into()));
        }

        let record = BlobRecord {
            file_number: self.file_number,
            offset: self.offset,
            cf_id: header.cf_id,
            seq: header.seq,
            key: Bytes::from(key),
            value: Bytes::from(value),
        };
        self.offset = padded_end;
        Ok(Some(record))
    }
}

fn blob_file_path(dir: &Path, file_number: FileNumber) -> PathBuf {
    dir.join(format!("{:06}.blob", file_number))
}

fn next_blob_file_number(dir: &Path) -> FileNumber {
    let mut max = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".blob")
                && let Ok(n) = stem.parse::<u64>()
            {
                max = max.max(n);
            }
        }
    }
    if max == 0 { 1 } else { max + 1 }
}

fn align_up(n: u64, align: u64) -> u64 {
    n.div_ceil(align) * align
}

#[cfg(unix)]
trait ReadExactAt {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()>;
}

#[cfg(unix)]
impl ReadExactAt for File {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        use std::os::unix::fs::FileExt;
        FileExt::read_exact_at(self, buf, offset)?;
        Ok(())
    }
}

#[cfg(not(unix))]
trait ReadExactAt {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()>;
}

#[cfg(not(unix))]
impl ReadExactAt for File {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = self;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)?;
        Ok(())
        // file is dropped here, closing the duplicated fd on non-Unix.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_ref_roundtrip() {
        let r = BlobRef {
            file_number: 7,
            offset: 1234,
            len: 56,
        };
        assert_eq!(BlobRef::decode(&r.encode()), Some(r));
    }

    #[test]
    fn blob_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path(), u64::MAX).unwrap();
        let value = b"a large value that would live in the blob log";
        let blob_ref = store.put(0, b"key", value, 1).unwrap();
        assert_eq!(store.get(blob_ref).unwrap().as_ref(), value);
    }

    #[test]
    fn blob_store_rotates_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path(), 1).unwrap();
        let r1 = store.put(0, b"x", b"x", 1).unwrap();
        let r2 = store.put(0, b"y", b"y", 2).unwrap();
        assert_ne!(r1.file_number, r2.file_number);
        assert_eq!(store.get(r1).unwrap().as_ref(), b"x");
        assert_eq!(store.get(r2).unwrap().as_ref(), b"y");
    }

    #[test]
    fn blob_corruption_detected() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path(), u64::MAX).unwrap();
        let blob_ref = store.put(0, b"key", b"hello", 1).unwrap();

        // Corrupt the value bytes.
        let path = blob_file_path(&dir.path().join(BLOB_DIR), blob_ref.file_number);
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            file.write_all_at(b"xxxxx", blob_ref.offset + BLOB_HEADER_SIZE + 3)
                .unwrap();
        }
        #[cfg(not(unix))]
        {
            use std::io::{Read, Seek, SeekFrom, Write};
            file.seek(SeekFrom::Start(blob_ref.offset + BLOB_HEADER_SIZE + 3))
                .unwrap();
            file.write_all(b"xxxxx").unwrap();
        }

        assert!(store.get(blob_ref).is_err());
    }

    #[test]
    fn blob_file_iterator_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path(), u64::MAX).unwrap();
        let refs = [
            store.put(0, b"a", b"value-a", 1).unwrap(),
            store.put(0, b"b", b"value-b", 2).unwrap(),
            store.put(0, b"c", b"value-c", 3).unwrap(),
        ];

        let mut iter = store.iter_file(refs[0].file_number).unwrap();
        let mut records = Vec::new();
        while let Some(rec) = iter.next_record().unwrap() {
            records.push((rec.key.to_vec(), rec.value.to_vec(), rec.seq));
        }
        assert_eq!(
            records,
            vec![
                (b"a".to_vec(), b"value-a".to_vec(), 1),
                (b"b".to_vec(), b"value-b".to_vec(), 2),
                (b"c".to_vec(), b"value-c".to_vec(), 3),
            ]
        );
    }

    struct DummyOwner {
        live: Vec<(ColumnFamilyId, Vec<u8>, BlobRef)>,
        rewritten: Vec<(ColumnFamilyId, Vec<u8>, Vec<u8>, SequenceNumber)>,
    }

    impl BlobOwner for DummyOwner {
        fn is_blob_live(
            &self,
            cf_id: ColumnFamilyId,
            key: &[u8],
            blob_ref: BlobRef,
            _snapshot: SequenceNumber,
        ) -> bool {
            self.live.iter().any(|(c, k, r)| {
                *c == cf_id && k.as_slice() == key && *r == blob_ref
            })
        }

        fn rewrite_blob(
            &mut self,
            cf_id: ColumnFamilyId,
            key: &[u8],
            value: &[u8],
            seq: SequenceNumber,
        ) -> Result<()> {
            self.rewritten.push((cf_id, key.to_vec(), value.to_vec(), seq));
            Ok(())
        }

        fn commit(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn gc_rewrites_live_and_drops_dead() {
        let dir = tempfile::tempdir().unwrap();
        // Threshold is large enough for two records but the third write rotates
        // the current file, leaving the first file non-current and eligible for
        // GC.
        let store = BlobStore::open(dir.path(), 200).unwrap();

        let live_value = vec![b'x'; 100];
        let dead_value = vec![b'y'; 100];
        let r1 = store.put(0, b"live", &live_value, 1).unwrap();
        let _r2 = store.put(0, b"dead", &dead_value, 2).unwrap();
        // Force a rotation so file 1 is no longer current.
        let _r3 = store.put(0, b"other", b"z", 3).unwrap();
        assert_ne!(r1.file_number, store.current_file_number());

        let mut owner = DummyOwner {
            live: vec![(0, b"live".to_vec(), r1)],
            rewritten: Vec::new(),
        };

        let stats = store
            .gc_once(&mut owner, &GcOptions { min_live_ratio: 0.9 }, 10)
            .unwrap();

        // One live record is rewritten, one dead record is dropped, and the
        // file is deleted once the commit succeeds.
        assert_eq!(stats.scanned_files, 1);
        assert_eq!(stats.rewritten_records, 1);
        assert_eq!(stats.dead_records, 1);
        assert_eq!(stats.deleted_files, 1);
        assert_eq!(owner.rewritten.len(), 1);
        assert_eq!(owner.rewritten[0].1, b"live");
    }

    #[test]
    fn gc_keeps_active_read_file_pending() {
        let dir = tempfile::tempdir().unwrap();
        // Each write goes to its own file; the first file is eligible for GC.
        let store = BlobStore::open(dir.path(), 1).unwrap();

        let r1 = store.put(0, b"k", b"v", 1).unwrap();
        let _r2 = store.put(0, b"k2", b"v2", 2).unwrap();
        assert_ne!(r1.file_number, store.current_file_number());

        // Hold a read lease on the file so GC cannot delete it.
        let lease = store.acquire_lease(r1.file_number);

        let mut owner = DummyOwner {
            live: vec![],
            rewritten: Vec::new(),
        };
        let stats = store
            .gc_once(&mut owner, &GcOptions { min_live_ratio: 0.9 }, 10)
            .unwrap();

        // The file is rewritten (because no records are live) but deletion is
        // deferred.
        assert_eq!(stats.deleted_files, 0);
        assert!(blob_file_path(&store.path(), r1.file_number).exists());

        drop(lease);

        // A subsequent pass cleans up the pending file.
        let stats = store
            .gc_once(&mut owner, &GcOptions { min_live_ratio: 0.9 }, 10)
            .unwrap();
        assert_eq!(stats.deleted_files, 1);
        assert!(!blob_file_path(&store.path(), r1.file_number).exists());
    }
}
