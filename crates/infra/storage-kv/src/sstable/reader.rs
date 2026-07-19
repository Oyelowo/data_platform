//! SSTable reader.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;

use super::block::{BlockComparator, BlockIterator};
use super::filter::BloomFilterReader;
use super::format::{
    BLOCK_TRAILER_SIZE, BlockHandle, CompressionType, FOOTER_SIZE, Footer, MAX_BLOCK_SIZE, checksum,
};
use super::index::IndexIterator;
use crate::Result;
use crate::SequenceNumber;
use crate::cache::{BlockCacheKey, BlockCaches};
use crate::file::{FadviseAdvice, RandomAccessFile, StdRandomAccessFile};
use crate::internal_key::{
    RangeTombstone, build_internal_key, extract_user_key, parse_internal_key,
};
use crate::merge_iter::InternalIterator;

/// Reader for a single SSTable file.
pub struct SSTableReader {
    path: PathBuf,
    file: Arc<dyn RandomAccessFile>,
    file_len: u64,
    #[allow(dead_code)]
    footer: Footer,
    index_block: Bytes,
    filter_reader: BloomFilterReader,
    range_tombstones: Vec<RangeTombstone>,
    file_number: crate::FileNumber,
    caches: Option<BlockCaches>,
}

impl SSTableReader {
    /// Open an existing SSTable.
    pub fn open(
        path: impl AsRef<Path>,
        file_number: crate::FileNumber,
        caches: Option<BlockCaches>,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file: Arc<dyn RandomAccessFile> = Arc::new(StdRandomAccessFile::open(&path)?);
        let file_len = file.len()?;
        if file_len < FOOTER_SIZE as u64 {
            return Err(crate::Error::Sstable("file too short".into()));
        }
        let footer_buf = file.read_exact_at(file_len - FOOTER_SIZE as u64, FOOTER_SIZE)?;
        let footer = Footer::decode(&footer_buf)?;

        // Point lookups dominate the read pattern of a live table; the hint
        // is advisory and never changes results.
        let _ = file.fadvise(0, file_len, FadviseAdvice::Random);

        let index_block = read_block(
            &file,
            file_len,
            file_number,
            footer.index_handle,
            caches.as_ref(),
            true,
        )?;

        let meta_index = read_block(
            &file,
            file_len,
            file_number,
            footer.metaindex_handle,
            caches.as_ref(),
            true,
        )?;
        let filter_handle = find_filter_handle(&meta_index)?;
        let filter_data = read_block(
            &file,
            file_len,
            file_number,
            filter_handle,
            caches.as_ref(),
            true,
        )?;

        let bits_per_key = find_bloom_bits_per_key(&meta_index).unwrap_or(10) as usize;
        let filter_reader = BloomFilterReader::new(&filter_data, bits_per_key);

        let range_tombstones = match find_range_tombstone_handle(&meta_index) {
            Some(handle) => {
                let rt_block =
                    read_block(&file, file_len, file_number, handle, caches.as_ref(), true)?;
                decode_range_tombstone_block(&rt_block)
            }
            None => Vec::new(),
        };

        Ok(Self {
            path,
            file,
            file_len,
            footer,
            index_block,
            filter_reader,
            range_tombstones,
            file_number,
            caches,
        })
    }

    /// Look up a key as of `snapshot_seq`.
    ///
    /// Returns `Some(Some(value))` if found, `Some(None)` for a tombstone, or
    /// `None` if the key is not present in the snapshot.
    pub fn get(
        &mut self,
        key: &[u8],
        snapshot_seq: SequenceNumber,
    ) -> Result<Option<Option<Bytes>>> {
        self.get_with_type(key, snapshot_seq)
            .map(|r| r.map(|opt| opt.map(|(_, val)| val)))
    }

    /// Look up a key as of `snapshot_seq`, returning the value type as well.
    ///
    /// Returns `Some(Some((ty, value)))` if found, `Some(None)` for a tombstone,
    /// or `None` if the key is not present in the snapshot.
    pub fn get_with_type(
        &mut self,
        key: &[u8],
        snapshot_seq: SequenceNumber,
    ) -> Result<Option<Option<(crate::internal_key::ValueType, Bytes)>>> {
        use crate::internal_key::ValueType;

        // Check range tombstones first so that a point read can be short-
        // circuited when the key is deleted by a range tombstone.  The filter
        // does not index range tombstones, so this check is cheap.
        let tombstone_seq = self.newest_covering_tombstone(key, snapshot_seq);

        if !self.filter_reader.may_contain(key) {
            // The filter says the key is absent as a point entry.  If a range
            // tombstone covers it, the key is deleted.
            return Ok(tombstone_seq.map(|_| None));
        }

        // Seek the index to the user key.  The index separators are user-key
        // boundaries encoded as raw bytes, so a plain user-key target works.
        let mut index_iter = IndexIterator::new(self.index_block.clone());
        index_iter.seek(key);
        if !index_iter.valid() {
            return Ok(tombstone_seq.map(|_| None));
        }
        let handle = index_iter.value();
        let data_block = read_block(
            &self.file,
            self.file_len,
            self.file_number,
            handle,
            self.caches.as_ref(),
            true,
        )?;

        // Seek to the newest version of `key`.  Using a max-sequence internal
        // key positions the block iterator at the first entry for this user key
        // (the block stores entries newest-first).
        let mut block_iter = BlockIterator::new(data_block, BlockComparator::InternalKey);
        let seek_key = build_internal_key(key, u64::MAX, ValueType::Value);
        block_iter.seek(&seek_key);
        if !block_iter.valid() {
            return Ok(tombstone_seq.map(|_| None));
        }

        // The block stores entries for the same user key in descending sequence
        // order (newest first).  The first entry with sequence <= snapshot_seq
        // is the newest visible version.
        let mut point_result: Option<(SequenceNumber, ValueType, Option<Bytes>)> = None;
        while block_iter.valid() {
            if extract_user_key(block_iter.key()) != key {
                break;
            }
            if let Some((seq, ty)) = parse_internal_key(block_iter.key())
                && seq <= snapshot_seq
            {
                let value = block_iter.value();
                point_result = Some((
                    seq,
                    ty,
                    if value.is_empty() {
                        None
                    } else {
                        Some(Bytes::copy_from_slice(value))
                    },
                ));
                break;
            }
            block_iter.next();
        }

        match (point_result, tombstone_seq) {
            (Some((point_seq, _, _)), Some(t_seq)) if t_seq >= point_seq => Ok(Some(None)),
            (Some((_, ValueType::Deletion, _)), _) => Ok(Some(None)),
            (Some((_, ty, Some(val))), _) => Ok(Some(Some((ty, val)))),
            (Some((_, ty, None)), _) => {
                // Defensive: a non-deletion entry should always carry a value.
                // Treat it as an empty inline value rather than returning an
                // error so that forward iteration is not interrupted.
                Ok(Some(Some((ty, Bytes::new()))))
            }
            (None, Some(_)) => Ok(Some(None)),
            (None, None) => Ok(None),
        }
    }

    /// Return an iterator over all entries.
    pub fn iter(&self) -> Result<SSTableIterator> {
        self.iter_fill(true)
    }

    /// Return an iterator over all entries, controlling whether data blocks
    /// are admitted into the block caches.
    ///
    /// Bulk consumers such as compaction pass `fill_cache = false`: they scan
    /// every table exactly once, so caching their blocks would only evict
    /// blocks that point lookups actually reuse.  The OS is additionally told
    /// the file pages are single-use via `fadvise`.
    pub fn iter_fill(&self, fill_cache: bool) -> Result<SSTableIterator> {
        SSTableIterator::new(self, fill_cache)
    }

    /// Return the path to the SSTable file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the bloom filter bits-per-key configured for this table.
    pub fn bloom_bits_per_key(&self) -> usize {
        self.filter_reader.bits_per_key()
    }
}

/// Iterator over an SSTable.
///
/// Entries are stored on disk in internal-key order: user key ascending,
/// sequence descending (newest first).  This iterator buffers the versions of
/// each user key and yields them in the logical order expected by the merge
/// iterator: user key ascending, sequence descending (newest first).
pub struct SSTableIterator {
    file: Arc<dyn RandomAccessFile>,
    file_len: u64,
    file_number: crate::FileNumber,
    caches: Option<BlockCaches>,
    fill_cache: bool,
    index_iter: IndexIterator,
    current_block: Option<(BlockHandle, Bytes, BlockIterator)>,
    /// Buffered entries for the current user key, in on-disk (sequence
    /// descending) order.  `fill_pending` reverses them so entries are popped
    /// from the back to yield newest first.
    pending: Vec<(Vec<u8>, Bytes)>,
}

impl SSTableIterator {
    fn new(reader: &SSTableReader, fill_cache: bool) -> Result<Self> {
        let index_iter = IndexIterator::new(reader.index_block.clone());
        // Iteration is sequential across the whole table (scans, compaction);
        // re-hint accordingly.  Advisory only.  When the caller will not
        // reuse the data (compaction), tell the OS the pages are single-use
        // so they do not pollute the page cache either.
        let advice = if fill_cache {
            FadviseAdvice::Sequential
        } else {
            FadviseAdvice::NoReuse
        };
        let _ = reader.file.fadvise(0, reader.file_len, advice);
        Ok(Self {
            file: Arc::clone(&reader.file),
            file_len: reader.file_len,
            file_number: reader.file_number,
            caches: reader.caches.clone(),
            fill_cache,
            index_iter,
            current_block: None,
            pending: Vec::new(),
        })
    }

    pub fn seek_to_first(&mut self) -> Result<()> {
        self.index_iter.seek_to_first();
        self.load_block()?;
        self.fill_pending()
    }

    pub fn seek(&mut self, target: &[u8]) -> Result<()> {
        // Index separators are user-key boundaries (or full internal keys for a
        // single-block table), so seek the index using the user-key portion of
        // the target.  The data block is then seeked with the full internal key.
        let user_target = extract_user_key(target);
        self.index_iter.seek(user_target);
        self.load_block()?;
        if let Some((_, _, ref mut bi)) = self.current_block {
            bi.seek(target);
        }
        self.fill_pending()
    }

    fn load_block(&mut self) -> Result<()> {
        if !self.index_iter.valid() {
            self.current_block = None;
            return Ok(());
        }
        let handle = self.index_iter.value();
        let data = read_block(
            &self.file,
            self.file_len,
            self.file_number,
            handle,
            self.caches.as_ref(),
            self.fill_cache,
        )?;
        let mut bi = BlockIterator::new(data.clone(), BlockComparator::InternalKey);
        bi.seek_to_first();
        self.current_block = Some((handle, data, bi));
        Ok(())
    }

    /// Read the next user key's worth of entries from the underlying block
    /// iterator into `pending`, then reverse so `pop()` yields newest first.
    fn fill_pending(&mut self) -> Result<()> {
        self.pending.clear();
        loop {
            let mut entries = match self.current_block {
                Some((_, _, ref mut bi)) if bi.valid() => {
                    let first_key = extract_user_key(bi.key()).to_vec();
                    let mut entries = Vec::new();
                    while bi.valid() && extract_user_key(bi.key()) == first_key {
                        entries.push((bi.key().to_vec(), bi.value_bytes()));
                        bi.next();
                    }
                    entries
                }
                _ => Vec::new(),
            };
            if !entries.is_empty() {
                entries.reverse();
                self.pending = entries;
                return Ok(());
            }
            self.advance_block()?;
            if self.current_block.is_none() {
                return Ok(());
            }
        }
    }

    fn advance_block(&mut self) -> Result<()> {
        loop {
            self.index_iter.next();
            self.load_block()?;
            match self.current_block {
                Some((_, _, ref bi)) if bi.valid() => return Ok(()),
                Some(_) => continue,
                None => return Ok(()),
            }
        }
    }

    pub fn key(&self) -> &[u8] {
        &self.pending.last().expect("invalid iterator").0
    }

    pub fn value(&self) -> &[u8] {
        &self.pending.last().expect("invalid iterator").1
    }

    pub fn valid(&self) -> bool {
        !self.pending.is_empty()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<()> {
        self.pending.pop();
        if self.pending.is_empty() {
            self.fill_pending()?;
        }
        Ok(())
    }
}

impl InternalIterator for SSTableIterator {
    fn seek_to_first(&mut self) -> Result<()> {
        self.seek_to_first()
    }

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.seek(target)
    }

    fn next(&mut self) -> Result<()> {
        self.next()
    }

    fn valid(&self) -> bool {
        self.valid()
    }

    fn key(&self) -> &[u8] {
        self.key()
    }

    fn value(&self) -> &[u8] {
        self.value()
    }
}

/// Read and decode a single block, serving from and populating the two cache
/// tiers.
///
/// Lookup order: hot tier (decompressed blocks) → cold tier (stored bytes,
/// CRC already verified at insert time) → disk (bounds check, positioned
/// read, CRC verify, decompress).  Blocks are admitted into both tiers only
/// when `fill_cache` is set; bulk scans such as compaction pass `false` so a
/// one-time pass does not evict blocks that point lookups keep reusing.
fn read_block(
    file: &Arc<dyn RandomAccessFile>,
    file_len: u64,
    file_number: crate::FileNumber,
    handle: BlockHandle,
    caches: Option<&BlockCaches>,
    fill_cache: bool,
) -> Result<Bytes> {
    let cache_key = BlockCacheKey {
        file_number,
        offset: handle.offset,
    };

    if let Some(caches) = caches {
        if let Some(block) = caches.hot().get(cache_key) {
            caches.metrics().record_hot_hit();
            return Ok(block);
        }
        caches.metrics().record_hot_miss();
        if let Some(raw) = caches.cold().and_then(|cold| cold.get(cache_key)) {
            caches.metrics().record_cold_hit();
            let data = decompress_stored(raw)?;
            if fill_cache {
                caches.hot().insert(cache_key, data.clone());
            }
            return Ok(data);
        }
        caches.metrics().record_cold_miss();
    }

    let block_len = handle
        .size
        .checked_add(BLOCK_TRAILER_SIZE as u64)
        .ok_or_else(|| crate::Error::Sstable("invalid block handle size".into()))?;
    let end_offset = handle
        .offset
        .checked_add(block_len)
        .ok_or_else(|| crate::Error::Sstable("invalid block handle offset".into()))?;
    if end_offset > file_len {
        return Err(crate::Error::Sstable(format!(
            "block handle [{}..{}] extends past file length {}",
            handle.offset, end_offset, file_len
        )));
    }
    if handle.size > MAX_BLOCK_SIZE {
        return Err(crate::Error::Sstable(format!(
            "block size {} exceeds maximum {}",
            handle.size, MAX_BLOCK_SIZE
        )));
    }

    let start = std::time::Instant::now();
    let raw = file.read_exact_at(handle.offset, block_len as usize)?;
    if let Some(caches) = caches {
        caches.metrics().record_disk_read(start.elapsed());
    }
    let block = raw.slice(..handle.size as usize);
    let trailer = &raw[handle.size as usize..];

    let compression = CompressionType::from_u8(trailer[0])
        .ok_or_else(|| crate::Error::Sstable("unknown compression type".into()))?;
    let stored_crc = u32::from_le_bytes([trailer[1], trailer[2], trailer[3], trailer[4]]);
    // The CRC covers the stored (compressed) bytes, so corruption is caught
    // before any decompression is attempted.
    let computed_crc = checksum(&block);
    if stored_crc != computed_crc {
        return Err(crate::Error::Sstable("block checksum mismatch".into()));
    }

    let data = crate::compression::decompress_block(compression, block)?;
    if data.len() as u64 > MAX_BLOCK_SIZE {
        return Err(crate::Error::Sstable(format!(
            "decompressed block size {} exceeds maximum {}",
            data.len(),
            MAX_BLOCK_SIZE
        )));
    }

    if fill_cache && let Some(caches) = caches {
        // The raw bytes are inserted first: if the hot-tier insert then
        // evicts, the stored form is still available without a disk read.
        if let Some(cold) = caches.cold() {
            cold.insert(cache_key, raw);
        }
        caches.hot().insert(cache_key, data.clone());
    }

    Ok(data)
}

/// Decode a block served from the cold tier.  The CRC is deliberately not
/// re-verified: bytes only enter the cold tier after passing verification.
fn decompress_stored(raw: Bytes) -> Result<Bytes> {
    if raw.len() < BLOCK_TRAILER_SIZE {
        return Err(crate::Error::Sstable(
            "stored block shorter than trailer".into(),
        ));
    }
    let size = raw.len() - BLOCK_TRAILER_SIZE;
    let compression = CompressionType::from_u8(raw[size])
        .ok_or_else(|| crate::Error::Sstable("unknown compression type".into()))?;
    let data = crate::compression::decompress_block(compression, raw.slice(..size))?;
    if data.len() as u64 > MAX_BLOCK_SIZE {
        return Err(crate::Error::Sstable(format!(
            "decompressed block size {} exceeds maximum {}",
            data.len(),
            MAX_BLOCK_SIZE
        )));
    }
    Ok(data)
}

fn find_filter_handle(meta_index: &[u8]) -> Result<BlockHandle> {
    let mut iter = BlockIterator::new(
        Bytes::copy_from_slice(meta_index),
        BlockComparator::Bytewise,
    );
    iter.seek_to_first();
    while iter.valid() {
        if iter.key() == b"filter.bloom" {
            return Ok(BlockHandle::decode(iter.value()).0);
        }
        iter.next();
    }
    Err(crate::Error::Sstable("filter handle not found".into()))
}

fn find_bloom_bits_per_key(meta_index: &[u8]) -> Option<u64> {
    let mut iter = BlockIterator::new(
        Bytes::copy_from_slice(meta_index),
        BlockComparator::Bytewise,
    );
    iter.seek_to_first();
    while iter.valid() {
        if iter.key() == b"filter.bloom.bits_per_key" {
            let value = iter.value();
            if value.len() >= 8 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&value[..8]);
                return Some(u64::from_le_bytes(buf));
            }
        }
        iter.next();
    }
    None
}

fn find_range_tombstone_handle(meta_index: &[u8]) -> Option<BlockHandle> {
    let mut iter = BlockIterator::new(
        Bytes::copy_from_slice(meta_index),
        BlockComparator::Bytewise,
    );
    iter.seek_to_first();
    while iter.valid() {
        if iter.key() == b"range_tombstone" {
            return Some(BlockHandle::decode(iter.value()).0);
        }
        iter.next();
    }
    None
}

fn decode_range_tombstone_block(block: &[u8]) -> Vec<RangeTombstone> {
    let mut out = Vec::new();
    let mut iter = BlockIterator::new(Bytes::copy_from_slice(block), BlockComparator::Bytewise);
    iter.seek_to_first();
    while iter.valid() {
        if let Some(rt) = RangeTombstone::decode(iter.value()) {
            out.push(rt);
        }
        iter.next();
    }
    out
}

impl SSTableReader {
    /// Return the sequence number of the newest range tombstone that covers
    /// `key` and is visible to `snapshot_seq`, or `None` if there is none.
    fn newest_covering_tombstone(
        &self,
        key: &[u8],
        snapshot_seq: SequenceNumber,
    ) -> Option<SequenceNumber> {
        let mut best: Option<SequenceNumber> = None;
        for rt in &self.range_tombstones {
            if rt.seq > snapshot_seq {
                continue;
            }
            if rt.covers(key) && best.is_none_or(|b| rt.seq > b) {
                best = Some(rt.seq);
            }
        }
        best
    }

    /// Return the range tombstones stored in this SSTable.
    pub fn range_tombstones(&self) -> &[RangeTombstone] {
        &self.range_tombstones
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::BlockCache;
    use crate::internal_key::{ValueType, build_internal_key};
    use crate::metrics::Metrics;
    use crate::sstable::builder::{SSTableBuilder, SSTableBuilderOptions};

    fn ik(user_key: &[u8]) -> Vec<u8> {
        build_internal_key(user_key, 1, ValueType::Value)
    }

    #[test]
    fn sstable_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();
        builder.add(&ik(b"a"), b"1").unwrap();
        builder.add(&ik(b"b"), b"2").unwrap();
        builder.add(&ik(b"c"), b"3").unwrap();
        builder.finish().unwrap();

        let mut reader = SSTableReader::open(&path, 1, None).unwrap();
        assert_eq!(
            reader.get(b"a", u64::MAX).unwrap(),
            Some(Some(Bytes::from_static(b"1")))
        );
        assert_eq!(
            reader.get(b"b", u64::MAX).unwrap(),
            Some(Some(Bytes::from_static(b"2")))
        );
        assert_eq!(reader.get(b"z", u64::MAX).unwrap(), None);
    }

    #[test]
    fn sstable_iterator_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();
        builder.add(&ik(b"a"), b"1").unwrap();
        builder.add(&ik(b"b"), b"2").unwrap();
        builder.add(&ik(b"c"), b"3").unwrap();
        builder.finish().unwrap();

        let reader = SSTableReader::open(&path, 1, None).unwrap();
        let mut iter = reader.iter().unwrap();
        iter.seek_to_first().unwrap();
        assert!(iter.valid());
        assert_eq!(iter.key(), ik(b"a"));
        assert_eq!(iter.value(), b"1");
        iter.next().unwrap();
        assert_eq!(iter.key(), ik(b"b"));
        iter.next().unwrap();
        assert_eq!(iter.key(), ik(b"c"));
        iter.next().unwrap();
        assert!(!iter.valid());
    }

    #[test]
    fn sstable_iterator_seek() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();
        builder.add(&ik(b"a"), b"1").unwrap();
        builder.add(&ik(b"c"), b"2").unwrap();
        builder.add(&ik(b"e"), b"3").unwrap();
        builder.finish().unwrap();

        let reader = SSTableReader::open(&path, 1, None).unwrap();
        let mut iter = reader.iter().unwrap();
        iter.seek(&ik(b"d")).unwrap();
        assert!(iter.valid());
        assert_eq!(iter.key(), ik(b"e"));
    }

    #[test]
    fn block_cache_reduces_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();
        for i in 0..100u16 {
            builder
                .add(&ik(&i.to_be_bytes()), &i.to_le_bytes())
                .unwrap();
        }
        builder.finish().unwrap();

        let cache = Arc::new(BlockCache::with_capacity(1024 * 1024));
        let caches = BlockCaches::new(cache.clone(), None, Arc::new(Metrics::default()));
        let mut reader = SSTableReader::open(&path, 1, Some(caches)).unwrap();

        // First get should populate the cache.
        assert!(
            reader
                .get(&50u16.to_be_bytes(), u64::MAX)
                .unwrap()
                .is_some()
        );
        // Second get for the same key should hit the cache.
        assert!(
            reader
                .get(&50u16.to_be_bytes(), u64::MAX)
                .unwrap()
                .is_some()
        );

        // The cache should contain at least one block.
        assert!(
            cache.total_weight() > 0,
            "at least one block should be cached"
        );
    }

    /// Build a table of 300 keys with small, well-compressing blocks and
    /// return its path.
    fn build_many_block_table(dir: &tempfile::TempDir) -> PathBuf {
        let path = dir.path().join("many.sst");
        let opts = SSTableBuilderOptions {
            block_size: 256,
            block_restart_interval: 16,
            bloom_bits_per_key: 10,
            compression: CompressionType::Lz4,
        };
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();
        for i in 0..300u32 {
            let key = format!("key{:04}", i);
            let value = vec![b'x'; 100];
            builder.add(&ik(key.as_bytes()), &value).unwrap();
        }
        builder.finish().unwrap();
        path
    }

    /// A cold-tier hit must serve the block without touching disk: after the
    /// file is corrupted underneath the reader, a key in an already-read block
    /// still resolves (no CRC re-verification, no I/O), while a key in an
    /// unread block fails integrity checks.
    #[test]
    fn cold_tier_serves_reads_when_hot_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = build_many_block_table(&dir);

        // Hot tier that can never retain anything; cold tier large enough.
        let hot = Arc::new(BlockCache::with_capacity(1));
        let cold = Arc::new(BlockCache::with_capacity(1 << 20));
        let caches = BlockCaches::new(
            hot.clone(),
            Some(cold.clone()),
            Arc::new(Metrics::default()),
        );
        let mut reader = SSTableReader::open(&path, 1, Some(caches)).unwrap();

        // Populate the cold tier with the block containing key0000.
        assert!(
            reader.get(b"key0000", u64::MAX).unwrap().is_some(),
            "first read should succeed"
        );
        assert!(
            cold.total_weight() > 0,
            "cold tier should hold stored bytes"
        );
        assert_eq!(
            hot.total_weight(),
            0,
            "hot tier of 1 byte must never retain a block"
        );

        // Corrupt the file in place. Keep the length identical so the change
        // isolates integrity checking from truncation handling.  Overwrite the
        // whole file so *every* unread block is corrupt while the cached block
        // is still served from memory.
        let file_bytes = std::fs::read(&path).unwrap();
        let junk = vec![0xFF; file_bytes.len()];
        std::fs::write(&path, junk).unwrap();

        // Cold-tier hit: served from memory, disk corruption irrelevant.
        assert!(
            reader.get(b"key0000", u64::MAX).unwrap().is_some(),
            "cold-tier hit must not touch the corrupted file"
        );

        // A key in a not-yet-read block must go to disk and fail there.
        assert!(
            reader.get(b"key0299", u64::MAX).is_err(),
            "cold-tier miss must reach disk and fail integrity checks"
        );
    }

    /// Compaction-style scans (`fill_cache = false`) must not admit blocks
    /// into either tier; point reads must.
    #[test]
    fn compaction_policy_does_not_fill_caches() {
        let dir = tempfile::tempdir().unwrap();
        let path = build_many_block_table(&dir);

        let hot = Arc::new(BlockCache::with_capacity(1 << 20));
        let cold = Arc::new(BlockCache::with_capacity(1 << 20));
        let caches = BlockCaches::new(
            hot.clone(),
            Some(cold.clone()),
            Arc::new(Metrics::default()),
        );
        let mut reader = SSTableReader::open(&path, 1, Some(caches)).unwrap();

        // open() legitimately caches the meta blocks; measure the scan as a
        // delta from that baseline.
        let hot_before = hot.total_weight();
        let cold_before = cold.total_weight();

        let mut iter = reader.iter_fill(false).unwrap();
        iter.seek_to_first().unwrap();
        let mut n = 0;
        while iter.valid() {
            n += 1;
            iter.next().unwrap();
        }
        assert_eq!(n, 300, "scan should visit every entry");

        assert_eq!(
            hot.total_weight(),
            hot_before,
            "fill_cache=false scan must not admit into the hot tier"
        );
        assert_eq!(
            cold.total_weight(),
            cold_before,
            "fill_cache=false scan must not admit into the cold tier"
        );

        // Point reads do fill.
        assert!(reader.get(b"key0150", u64::MAX).unwrap().is_some());
        assert!(
            hot.total_weight() > hot_before,
            "point read must admit its block into the hot tier"
        );
    }

    /// Reproduce the key distribution produced by the concurrent-write test:
    /// two threads write `t0-kN` and `t1-kN` with interleaved sequence numbers.
    /// The SSTable stores internal keys sorted by raw bytes (user-key ascending,
    /// sequence ascending). This test verifies the reader can locate every key.
    #[test]
    fn reader_finds_tn_kn_keys() {
        use crate::internal_key::{ValueType, build_internal_key};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();

        // Build the same 82-key set that the two-thread compaction produces.
        let mut keys: Vec<Vec<u8>> = Vec::new();
        for t in 0..2u32 {
            for i in 0..50u32 {
                let user_key = format!("t{}-k{}", t, i);
                let seq = (t * 50 + i + 1) as u64;
                keys.push(build_internal_key(
                    user_key.as_bytes(),
                    seq,
                    ValueType::Value,
                ));
            }
        }
        keys.sort();
        for (idx, ikey) in keys.iter().enumerate() {
            builder.add(ikey, format!("v{}", idx).as_bytes()).unwrap();
        }
        let built = builder.finish().unwrap();

        let mut reader = SSTableReader::open(&path, 1, None).unwrap();
        for t in 0..2u32 {
            for i in 0..50u32 {
                let user_key = format!("t{}-k{}", t, i);
                let got = reader.get(user_key.as_bytes(), u64::MAX).unwrap();
                assert!(
                    got.is_some(),
                    "missing key {} in sstable with {} keys smallest={:?} largest={:?}",
                    user_key,
                    built.num_entries,
                    std::str::from_utf8(&built.smallest_key).unwrap_or("?"),
                    std::str::from_utf8(&built.largest_key).unwrap_or("?")
                );
            }
        }
    }

    #[test]
    fn range_tombstone_roundtrip() {
        use crate::internal_key::RangeTombstone;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let mut builder = SSTableBuilder::open(
            &path,
            SSTableBuilderOptions {
                compression: CompressionType::None,
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..10u8 {
            builder.add(&ik(&[i]), &[i, 1]).unwrap();
        }
        builder
            .add_range_tombstone(RangeTombstone {
                start: vec![3],
                end: vec![7],
                seq: 100,
            })
            .unwrap();
        builder.finish().unwrap();

        let mut reader = SSTableReader::open(&path, 1, None).unwrap();
        assert_eq!(reader.range_tombstones().len(), 1);
        for i in 0..10u8 {
            let got = reader.get(&[i], u64::MAX).unwrap();
            if (3..7).contains(&i) {
                assert_eq!(got, Some(None), "key {} should be range-deleted", i);
            } else {
                assert_eq!(got, Some(Some(Bytes::from(vec![i, 1]))));
            }
        }
    }

    /// The persisted bloom filter bits-per-key must be read back and used to
    /// construct the reader.  With a non-default value, present keys must still
    /// never produce false negatives.
    #[test]
    fn bloom_bits_per_key_is_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let mut builder = SSTableBuilder::open(
            &path,
            SSTableBuilderOptions {
                bloom_bits_per_key: 4,
                compression: CompressionType::None,
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..200u16 {
            builder.add(&ik(&i.to_be_bytes()), &i.to_le_bytes()).unwrap();
        }
        builder.finish().unwrap();

        let mut reader = SSTableReader::open(&path, 1, None).unwrap();
        assert_eq!(reader.bloom_bits_per_key(), 4);
        for i in 0..200u16 {
            assert!(
                reader.get(&i.to_be_bytes(), u64::MAX).unwrap().is_some(),
                "false negative for key {} with persisted bits_per_key",
                i
            );
        }
    }

    /// Regression: multi-block SSTables must be fully readable. 800 keys
    /// spanning many data blocks exercises the index block and block-boundary
    /// logic.
    #[test]
    fn reader_finds_800_tn_kn_keys() {
        use crate::internal_key::{ValueType, build_internal_key};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();

        let mut keys: Vec<Vec<u8>> = Vec::new();
        for t in 0..8u32 {
            for i in 0..100u32 {
                let user_key = format!("t{}-k{}", t, i);
                let seq = (t * 100 + i + 1) as u64;
                keys.push(build_internal_key(
                    user_key.as_bytes(),
                    seq,
                    ValueType::Value,
                ));
            }
        }
        keys.sort();
        for (idx, ikey) in keys.iter().enumerate() {
            builder.add(ikey, format!("v{}", idx).as_bytes()).unwrap();
        }
        let built = builder.finish().unwrap();
        assert!(
            built.num_entries == 800,
            "expected 800 entries, got {}",
            built.num_entries
        );

        let mut reader = SSTableReader::open(&path, 1, None).unwrap();
        for t in 0..8u32 {
            for i in 0..100u32 {
                let user_key = format!("t{}-k{}", t, i);
                let got = reader.get(user_key.as_bytes(), u64::MAX).unwrap();
                assert!(
                    got.is_some(),
                    "missing key {} in {}-entry sstable",
                    user_key,
                    built.num_entries
                );
            }
        }
    }
}
