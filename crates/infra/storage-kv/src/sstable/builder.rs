//! SSTable builder.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Result;

use super::block::BlockBuilder;
use super::filter::BloomFilterBuilder;
use super::format::{BlockHandle, CompressionType, Footer, BLOCK_TRAILER_SIZE, checksum};
use super::index::IndexBuilder;

/// Options for building an SSTable.
#[derive(Debug, Clone, Copy)]
pub struct SSTableBuilderOptions {
    pub block_size: usize,
    pub block_restart_interval: usize,
    pub bloom_bits_per_key: usize,
}

impl Default for SSTableBuilderOptions {
    fn default() -> Self {
        Self {
            block_size: 4 * 1024,
            block_restart_interval: 16,
            bloom_bits_per_key: 10,
        }
    }
}

/// Builder that writes an SSTable to a file.
pub struct SSTableBuilder {
    path: PathBuf,
    file: File,
    options: SSTableBuilderOptions,
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
            self.index_builder.add_index_entry(&self.last_key, self.pending_handle);
            self.pending_index_entry = false;
        }

        self.filter_builder.add_key(key);
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

    fn write_block(&mut self, block: &[u8]) -> Result<BlockHandle> {
        let handle = BlockHandle {
            offset: self.offset,
            size: block.len() as u64,
        };
        self.file.write_all(block)?;

        let mut trailer = [0u8; BLOCK_TRAILER_SIZE];
        trailer[0] = CompressionType::None as u8;
        let crc = checksum(block);
        trailer[1..].copy_from_slice(&crc.to_le_bytes());
        self.file.write_all(&trailer)?;

        self.offset += block.len() as u64 + BLOCK_TRAILER_SIZE as u64;
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

        // Write filter block as a meta block.
        let filter_data = self.filter_builder.finish();
        let filter_handle = self.write_block(&filter_data)?;

        // Write meta-index block.
        let mut meta_index_builder = BlockBuilder::new(self.options.block_restart_interval);
        let mut handle_buf = Vec::new();
        filter_handle.encode(&mut handle_buf);
        meta_index_builder.add(b"filter.bloom", &handle_buf);
        let meta_index_block = meta_index_builder.finish().to_vec();
        let meta_index_handle = self.write_block(&meta_index_block)?;

        // Write index block.
        if self.pending_index_entry {
            self.index_builder.add_index_entry(&self.last_key, self.pending_handle);
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
}
