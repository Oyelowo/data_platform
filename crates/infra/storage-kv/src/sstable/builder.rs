//! SSTable builder.

use std::borrow::Cow;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::compression::{CompressionCodec, codec_for};
use crate::internal_key::{RangeTombstone, build_internal_key, extract_user_key, ValueType};

use super::block::BlockBuilder;
use super::filter::BloomFilterBuilder;
use super::format::{BLOCK_TRAILER_SIZE, BlockHandle, CompressionType, Footer, checksum};
use super::index::IndexBuilder;

/// Options for building an SSTable.
#[derive(Debug, Clone, Copy)]
pub struct SSTableBuilderOptions {
    pub block_size: usize,
    pub block_restart_interval: usize,
    pub bloom_bits_per_key: usize,
    /// Compression applied to data and index blocks.  Filter blocks are
    /// always stored uncompressed.
    pub compression: CompressionType,
}

impl Default for SSTableBuilderOptions {
    fn default() -> Self {
        Self {
            block_size: 4 * 1024,
            block_restart_interval: 16,
            bloom_bits_per_key: 10,
            compression: CompressionType::Lz4,
        }
    }
}

/// Builder that writes an SSTable to a file.
pub struct SSTableBuilder {
    path: PathBuf,
    file: File,
    options: SSTableBuilderOptions,
    codec: Option<Box<dyn CompressionCodec>>,
    data_block: BlockBuilder,
    index_builder: IndexBuilder,
    filter_builder: BloomFilterBuilder,
    last_key: Vec<u8>,
    pending_index_entry: bool,
    pending_handle: BlockHandle,
    offset: u64,
    num_entries: u64,
    smallest_key: Option<Vec<u8>>,
    largest_key: Option<Vec<u8>>,
    uncompressed_bytes: u64,
    compressed_bytes: u64,
    range_tombstones: Vec<RangeTombstone>,
}

impl SSTableBuilder {
    /// Create a new builder that writes to `path`.
    pub fn open(path: impl AsRef<Path>, options: SSTableBuilderOptions) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(&path)?;
        Ok(Self {
            path,
            file,
            options,
            codec: codec_for(options.compression),
            data_block: BlockBuilder::new(options.block_restart_interval),
            index_builder: IndexBuilder::new(options.block_restart_interval),
            filter_builder: BloomFilterBuilder::new(options.bloom_bits_per_key),
            last_key: Vec::new(),
            pending_index_entry: false,
            pending_handle: BlockHandle::default(),
            offset: 0,
            num_entries: 0,
            smallest_key: None,
            largest_key: None,
            uncompressed_bytes: 0,
            compressed_bytes: 0,
            range_tombstones: Vec::new(),
        })
    }

    /// Add a key/value pair. Keys must be added in ascending order.
    pub fn add(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if !self.last_key.is_empty() && self.last_key.as_slice() >= key {
            return Err(crate::Error::InvalidArgument(
                "keys must be added in ascending order".into(),
            ));
        }

        if self.pending_index_entry {
            // The previous data block ended at `self.last_key`. The current key
            // is the first key of the next data block, so the index separator
            // must lie between them.
            self.index_builder
                .add_index_entry(&self.last_key, key, self.pending_handle);
            self.pending_index_entry = false;
        }

        // Bloom filters are keyed by user key so that reads can test the user
        // key directly without decoding the internal-key trailer.
        self.filter_builder.add_key(extract_user_key(key));
        self.data_block.add(key, value);
        self.last_key = key.to_vec();
        self.num_entries += 1;

        if self.smallest_key.is_none() {
            self.smallest_key = Some(key.to_vec());
        }
        self.largest_key = Some(key.to_vec());

        if self.data_block.current_size_estimate() >= self.options.block_size {
            self.flush_data_block()?;
        }

        Ok(())
    }

    /// Add a range tombstone to the SSTable's range-deletion meta-block.
    ///
    /// Tombstones must be added in ascending `start` order; overlapping
    /// tombstones from different sequence numbers are allowed and are resolved
    /// at read time by sequence number.
    pub fn add_range_tombstone(&mut self, tombstone: RangeTombstone) -> Result<()> {
        if !self.range_tombstones.is_empty()
            && self.range_tombstones.last().unwrap().start > tombstone.start
        {
            return Err(crate::Error::InvalidArgument(
                "range tombstones must be added in ascending start order".into(),
            ));
        }

        // Update file bounds so that compaction overlap considers the full
        // span covered by range tombstones, not just the point keys.
        let start_ikey = build_internal_key(&tombstone.start, tombstone.seq, ValueType::RangeDeletion);
        let end_ikey = build_internal_key(&tombstone.end, tombstone.seq, ValueType::RangeDeletion);
        if self.smallest_key.is_none()
            || start_ikey < self.smallest_key.as_ref().unwrap().clone()
        {
            self.smallest_key = Some(start_ikey);
        }
        if self.largest_key.is_none()
            || end_ikey > self.largest_key.as_ref().unwrap().clone()
        {
            self.largest_key = Some(end_ikey);
        }

        self.range_tombstones.push(tombstone);
        Ok(())
    }

    fn flush_data_block(&mut self) -> Result<()> {
        if self.data_block.is_empty() {
            return Ok(());
        }
        let block = self.data_block.finish().to_vec();
        self.pending_handle = self.write_block(&block)?;
        self.pending_index_entry = true;
        self.data_block.reset();
        Ok(())
    }

    /// Write a block using the configured compression codec.  The codec
    /// itself decides per block whether compression is worthwhile, so the
    /// stored type may be `CompressionType::None` even when a codec is set.
    fn write_block(&mut self, block: &[u8]) -> Result<BlockHandle> {
        let (stored, ty) = match self.codec {
            Some(ref codec) => {
                let (encoded, ty) = codec.encode(block)?;
                // encode must report either its own type or the uncompressed
                // fallback; anything else would corrupt the trailer.
                debug_assert!(ty == codec.ty() || ty == CompressionType::None);
                (Cow::Owned(encoded), ty)
            }
            None => (Cow::Borrowed(block), CompressionType::None),
        };
        self.write_raw(block.len() as u64, &stored, ty)
    }

    /// Write a block without compression.  Used for the filter block, which
    /// RocksDB/LevelDB also store uncompressed: it is small, read on every
    /// lookup, and compressing it would only add latency.
    fn write_block_uncompressed(&mut self, block: &[u8]) -> Result<BlockHandle> {
        self.write_raw(block.len() as u64, block, CompressionType::None)
    }

    fn write_raw(&mut self, uncompressed_len: u64, stored: &[u8], ty: CompressionType) -> Result<BlockHandle> {
        let handle = BlockHandle {
            offset: self.offset,
            size: stored.len() as u64,
        };
        self.file.write_all(stored)?;

        let mut trailer = [0u8; BLOCK_TRAILER_SIZE];
        trailer[0] = ty as u8;
        let crc = checksum(stored);
        trailer[1..].copy_from_slice(&crc.to_le_bytes());
        self.file.write_all(&trailer)?;

        self.offset += stored.len() as u64 + BLOCK_TRAILER_SIZE as u64;
        self.uncompressed_bytes += uncompressed_len;
        self.compressed_bytes += stored.len() as u64;
        Ok(handle)
    }

    /// Approximate current file size of the SSTable being built.
    pub fn current_size_estimate(&self) -> usize {
        self.offset as usize
            + self.data_block.current_size_estimate()
            + self.index_builder.current_size_estimate()
    }

    /// Finish the SSTable and return metadata.
    pub fn finish(mut self) -> Result<BuiltSSTable> {
        self.flush_data_block()?;

        // Write filter block as a meta block.  Filter blocks are never
        // compressed: they are small and read on nearly every lookup.
        let filter_data = self.filter_builder.finish();
        let filter_handle = self.write_block_uncompressed(&filter_data)?;

        // Write range tombstone meta-block (uncompressed; read on every open).
        let range_tombstone_handle = if self.range_tombstones.is_empty() {
            None
        } else {
            let mut rt_builder = BlockBuilder::new(self.options.block_restart_interval);
            for rt in &self.range_tombstones {
                rt_builder.add(&rt.start, &rt.encode());
            }
            let rt_block = rt_builder.finish().to_vec();
            Some(self.write_block_uncompressed(&rt_block)?)
        };

        // Write meta-index block.
        let mut meta_index_builder = BlockBuilder::new(self.options.block_restart_interval);
        let mut handle_buf = Vec::new();
        filter_handle.encode(&mut handle_buf);
        meta_index_builder.add(b"filter.bloom", &handle_buf);
        if let Some(ref h) = range_tombstone_handle {
            handle_buf.clear();
            h.encode(&mut handle_buf);
            meta_index_builder.add(b"range_tombstone", &handle_buf);
        }
        let meta_index_block = meta_index_builder.finish().to_vec();
        let meta_index_handle = self.write_block(&meta_index_block)?;

        // Write index block.
        if self.pending_index_entry {
            // For the last data block there is no following key, so the
            // separator is exactly the block's last key.
            self.index_builder
                .add_index_entry(&self.last_key, &self.last_key, self.pending_handle);
            self.pending_index_entry = false;
        }
        let index_block = self.index_builder.finish().to_vec();
        let index_handle = self.write_block(&index_block)?;

        // Write footer.
        let footer = Footer {
            metaindex_handle: meta_index_handle,
            index_handle,
        };
        let mut footer_buf = Vec::new();
        footer.encode(&mut footer_buf);
        self.file.write_all(&footer_buf)?;
        self.file.sync_all()?;

        Ok(BuiltSSTable {
            path: self.path,
            smallest_key: self.smallest_key.unwrap_or_default(),
            largest_key: self.largest_key.unwrap_or_default(),
            num_entries: self.num_entries,
            file_size: self.offset + footer_buf.len() as u64,
            uncompressed_bytes: self.uncompressed_bytes,
            compressed_bytes: self.compressed_bytes,
        })
    }
}

/// Metadata for a finished SSTable.
#[derive(Debug, Clone)]
pub struct BuiltSSTable {
    pub path: PathBuf,
    pub smallest_key: Vec<u8>,
    pub largest_key: Vec<u8>,
    pub num_entries: u64,
    pub file_size: u64,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
}
