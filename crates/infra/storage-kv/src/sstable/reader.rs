//! SSTable reader.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bytes::Bytes;

use crate::Result;
use super::block::BlockIterator;
use super::filter::BloomFilterReader;
use super::format::{BlockHandle, CompressionType, Footer, FOOTER_SIZE, BLOCK_TRAILER_SIZE, checksum};
use super::index::IndexIterator;

/// Reader for a single SSTable file.
pub struct SSTableReader {
    path: PathBuf,
    file: File,
    #[allow(dead_code)]
    footer: Footer,
    index_block: Vec<u8>,
    filter_reader: BloomFilterReader,
}

impl SSTableReader {
    /// Open an existing SSTable.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let file_len = file.metadata()?.len();
        if file_len < FOOTER_SIZE as u64 {
            return Err(crate::Error::Sstable("file too short".into()));
        }
        file.seek(SeekFrom::Start(file_len - FOOTER_SIZE as u64))?;
        let mut footer_buf = [0u8; FOOTER_SIZE];
        file.read_exact(&mut footer_buf)?;
        let footer = Footer::decode(&footer_buf)?;

        let index_block = read_block(&mut file, footer.index_handle)?;

        let meta_index = read_block(&mut file, footer.metaindex_handle)?;
        let filter_handle = find_filter_handle(&meta_index)?;
        let filter_data = read_block(&mut file, filter_handle)?;

        let filter_reader = BloomFilterReader::new(&filter_data, 10);

        Ok(Self {
            path,
            file,
            footer,
            index_block,
            filter_reader,
        })
    }

    /// Look up a key. Returns `Some(Some(value))` if found, `Some(None)` for a
    /// tombstone, or `None` if the key is not present.
    pub fn get(&mut self, key: &[u8]) -> Result<Option<Option<Bytes>>> {
        if !self.filter_reader.may_contain(key) {
            return Ok(None);
        }

        let mut index_iter = IndexIterator::new(&self.index_block);
        index_iter.seek(key);
        if !index_iter.valid() {
            return Ok(None);
        }
        let handle = index_iter.value();
        let data_block = read_block(&mut self.file, handle)?;

        let mut block_iter = BlockIterator::new(&data_block);
        block_iter.seek(key);
        if !block_iter.valid() {
            return Ok(None);
        }
        if block_iter.key() != key {
            return Ok(None);
        }

        let value = block_iter.value();
        if value.is_empty() {
            Ok(Some(None))
        } else {
            Ok(Some(Some(Bytes::copy_from_slice(value))))
        }
    }

    /// Return an iterator over all entries.
    pub fn iter(&mut self) -> Result<SSTableIterator<'_>> {
        SSTableIterator::new(self)
    }

    /// Return the path to the SSTable file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Iterator over an SSTable.
pub struct SSTableIterator<'a> {
    reader: &'a mut SSTableReader,
    #[allow(dead_code)]
    index_data: Vec<u8>,
    index_iter: IndexIterator<'a>,
    block_data: Vec<u8>,
    block_iter: Option<BlockIterator<'a>>,
}

impl<'a> SSTableIterator<'a> {
    fn new(reader: &'a mut SSTableReader) -> Result<Self> {
        let index_data = reader.index_block.clone();
        // SAFETY: index_data is owned by self and lives as long as 'a.
        let index_slice: &'a [u8] = unsafe { std::mem::transmute(&index_data[..]) };
        let index_iter = IndexIterator::new(index_slice);
        Ok(Self {
            reader,
            index_data,
            index_iter,
            block_data: Vec::new(),
            block_iter: None,
        })
    }

    pub fn seek_to_first(&mut self) -> Result<()> {
        self.index_iter.seek_to_first();
        self.load_block()
    }

    pub fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.index_iter.seek(target);
        self.load_block()?;
        if let Some(ref mut bi) = self.block_iter {
            bi.seek(target);
        }
        Ok(())
    }

    fn load_block(&mut self) -> Result<()> {
        if !self.index_iter.valid() {
            self.block_iter = None;
            return Ok(());
        }
        let handle = self.index_iter.value();
        self.block_data = read_block(&mut self.reader.file, handle)?;
        // SAFETY: block_data is owned by self and not mutated while iterator exists.
        let data: &'a [u8] = unsafe { std::mem::transmute(&self.block_data[..]) };
        let mut bi = BlockIterator::new(data);
        bi.seek_to_first();
        self.block_iter = Some(bi);
        Ok(())
    }

    pub fn key(&self) -> &[u8] {
        self.block_iter.as_ref().expect("invalid iterator").key()
    }

    pub fn value(&self) -> &[u8] {
        self.block_iter.as_ref().expect("invalid iterator").value()
    }

    pub fn valid(&self) -> bool {
        self.block_iter.as_ref().map(|b| b.valid()).unwrap_or(false)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<()> {
        if let Some(ref mut bi) = self.block_iter {
            bi.next();
            if !bi.valid() {
                self.index_iter.next();
                self.load_block()?;
            }
        }
        Ok(())
    }
}

fn read_block(file: &mut File, handle: BlockHandle) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(handle.offset))?;
    let mut buf = vec![0u8; handle.size as usize + BLOCK_TRAILER_SIZE];
    file.read_exact(&mut buf)?;

    let block = &buf[..handle.size as usize];
    let compression = CompressionType::from_u8(buf[handle.size as usize])
        .ok_or_else(|| crate::Error::Sstable("unknown compression type".into()))?;
    let stored_crc = u32::from_le_bytes([
        buf[handle.size as usize + 1],
        buf[handle.size as usize + 2],
        buf[handle.size as usize + 3],
        buf[handle.size as usize + 4],
    ]);
    let computed_crc = checksum(block);
    if stored_crc != computed_crc {
        return Err(crate::Error::Sstable("block checksum mismatch".into()));
    }

    match compression {
        CompressionType::None => Ok(block.to_vec()),
    }
}

fn find_filter_handle(meta_index: &[u8]) -> Result<BlockHandle> {
    let mut iter = BlockIterator::new(meta_index);
    iter.seek_to_first();
    while iter.valid() {
        if iter.key() == b"filter.bloom" {
            return Ok(BlockHandle::decode(iter.value()).0);
        }
        iter.next();
    }
    Err(crate::Error::Sstable("filter handle not found".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sstable::builder::{SSTableBuilder, SSTableBuilderOptions};

    #[test]
    fn sstable_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(&path, opts).unwrap();
        builder.add(b"a", b"1").unwrap();
        builder.add(b"b", b"2").unwrap();
        builder.add(b"c", b"3").unwrap();
        builder.finish().unwrap();

        let mut reader = SSTableReader::open(&path).unwrap();
        assert_eq!(reader.get(b"a").unwrap(), Some(Some(Bytes::from_static(b"1"))));
        assert_eq!(reader.get(b"b").unwrap(), Some(Some(Bytes::from_static(b"2"))));
        assert_eq!(reader.get(b"z").unwrap(), None);
    }
}
