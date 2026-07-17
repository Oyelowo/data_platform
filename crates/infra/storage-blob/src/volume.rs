//! Append-only volume file writer and reader.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;

use crate::format::{RecordHeader, HEADER_SIZE, padding_len};
use crate::{Error, Result};

/// Location of a single record inside a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordLocation {
    /// Volume file number.
    pub volume_number: u64,
    /// Byte offset of the record header in the volume.
    pub offset: u64,
    /// Total on-disk size of the record (header + id + payload + padding).
    pub record_size: u64,
}

/// A single append-only volume file.
#[derive(Debug)]
pub struct VolumeWriter {
    number: u64,
    file: File,
    size: u64,
    path: PathBuf,
}

impl VolumeWriter {
    /// Create (or truncate) a volume file.
    pub fn create(path: impl AsRef<Path>, number: u64) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            number,
            file,
            size: 0,
            path,
        })
    }

    /// Volume number.
    pub fn number(&self) -> u64 {
        self.number
    }

    /// Current committed size of the volume.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Path to the volume file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a record for `id` and the payload read from `reader`.
    ///
    /// Returns the location of the written record and the record header.
    /// The caller must serialize volume appends (one active writer per store).
    pub fn append_record(&mut self, id: &[u8], reader: &mut dyn Read) -> Result<(RecordLocation, RecordHeader)> {
        let offset = self.size;
        let id_len = id.len() as u32;

        // Reserve space for the header.
        let placeholder = [0u8; HEADER_SIZE];
        self.file.write_all(&placeholder)?;

        // Write the object ID.
        self.file.write_all(id)?;

        // Stream the payload, computing CRC and length.
        let mut crc: u32 = 0;
        let mut payload_len: u64 = 0;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            self.file.write_all(&buf[..n])?;
            crc = crc32c::crc32c_append(crc, &buf[..n]);
            payload_len += n as u64;
        }

        // Write padding to 8-byte alignment.
        let pad = padding_len(id_len, payload_len);
        if pad > 0 {
            let zeros = vec![0u8; pad as usize];
            self.file.write_all(&zeros)?;
        }

        let record_size = HEADER_SIZE as u64 + id_len as u64 + payload_len + pad;

        // Seek back and write the real header.
        self.file.seek(SeekFrom::Start(offset))?;
        let header = RecordHeader::new(id_len, payload_len, crc);
        let mut header_buf = [0u8; HEADER_SIZE];
        header.encode(&mut header_buf);
        self.file.write_all(&header_buf)?;

        // Return to the end of the record.
        self.file.seek(SeekFrom::Start(offset + record_size))?;
        self.size = offset + record_size;

        let location = RecordLocation {
            volume_number: self.number,
            offset,
            record_size,
        };
        Ok((location, header))
    }

    /// Flush the volume file to stable storage.
    pub fn sync(&self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    /// Truncate the volume to `size` bytes and reset the append pointer.
    pub fn truncate(&mut self, size: u64) -> Result<()> {
        self.file.set_len(size)?;
        self.file.seek(SeekFrom::Start(size))?;
        self.size = size;
        Ok(())
    }
}

/// Reader for an existing volume file.
#[derive(Debug)]
pub struct VolumeReader {
    number: u64,
    file: File,
    path: PathBuf,
}

impl VolumeReader {
    /// Open an existing volume file for reading.
    pub fn open(path: impl AsRef<Path>, number: u64) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new().read(true).open(&path)?;
        Ok(Self {
            number,
            file,
            path,
        })
    }

    /// Volume number.
    pub fn number(&self) -> u64 {
        self.number
    }

    /// Path to the volume file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read and validate the record header at `offset`.
    ///
    /// Returns the header and the total record size.
    pub fn read_header(&self, offset: u64) -> Result<(RecordHeader, u64)> {
        let mut header_buf = [0u8; HEADER_SIZE];
        Self::read_exact_at(&self.file, offset, &mut header_buf)?;
        let header = RecordHeader::decode(&header_buf)?;
        Ok((header, header.record_size()))
    }

    /// Read the full record (header + id + payload) at `offset` into memory.
    ///
    /// The payload CRC is verified.  This is useful for recovery and GC.
    pub fn read_record(&self, offset: u64) -> Result<(RecordHeader, Bytes, Bytes)> {
        let (header, record_size) = self.read_header(offset)?;
        let mut record_buf = vec![0u8; record_size as usize];
        Self::read_exact_at(&self.file, offset, &mut record_buf)?;

        let id_start = HEADER_SIZE;
        let id_end = id_start + header.id_len as usize;
        let payload_start = id_end;
        let payload_end = payload_start + header.payload_len as usize;

        let id = Bytes::copy_from_slice(&record_buf[id_start..id_end]);
        let payload = Bytes::copy_from_slice(&record_buf[payload_start..payload_end]);

        let actual_crc = crc32c::crc32c(&payload);
        if actual_crc != header.payload_crc {
            return Err(Error::CorruptRecord {
                volume: self.number,
                offset,
                message: format!(
                    "payload crc mismatch: expected {:08x}, got {:08x}",
                    header.payload_crc, actual_crc
                ),
            });
        }

        Ok((header, id, payload))
    }

    /// Return the total file size.
    pub fn file_size(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    /// Iterate over all valid records in the volume, stopping at the first
    /// corrupt or partial record (which recovery will truncate).
    pub fn iter(&self) -> VolumeRecordIterator<'_> {
        VolumeRecordIterator {
            reader: self,
            offset: 0,
        }
    }

    fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            file.read_exact_at(buf, offset)?;
        }
        #[cfg(not(unix))]
        {
            // Fallback for non-Unix platforms: this is not production-tuned.
            use std::io::Seek;
            let mut file = file.try_clone()?;
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(buf)?;
        }
        Ok(())
    }
}

/// Streaming reader for a single object's payload.
pub struct BlobPayloadReader {
    file: File,
    remaining: u64,
    crc: u32,
    expected_crc: u32,
    pending_verify: bool,
}

impl BlobPayloadReader {
    /// Create a reader for the payload of `header` located at `offset` in `path`.
    /// Verifies that the stored ID matches `expected_id`.
    pub fn open(
        path: &Path,
        offset: u64,
        header: &RecordHeader,
        expected_id: &[u8],
    ) -> Result<Self> {
        let mut file = OpenOptions::new().read(true).open(path)?;

        // Verify the stored ID matches the requested ID.
        let id_offset = offset + HEADER_SIZE as u64;
        file.seek(SeekFrom::Start(id_offset))?;
        let mut stored_id = vec![0u8; header.id_len as usize];
        file.read_exact(&mut stored_id)?;
        if stored_id.as_slice() != expected_id {
            return Err(Error::CorruptRecord {
                volume: 0,
                offset,
                message: "record id mismatch".into(),
            });
        }

        let payload_offset = offset + header.payload_offset();
        file.seek(SeekFrom::Start(payload_offset))?;
        Ok(Self {
            file,
            remaining: header.payload_len,
            crc: 0,
            expected_crc: header.payload_crc,
            pending_verify: false,
        })
    }
}

impl Read for BlobPayloadReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending_verify {
            if self.crc != self.expected_crc {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "payload crc mismatch: expected {:08x}, got {:08x}",
                        self.expected_crc, self.crc
                    ),
                ));
            }
            self.pending_verify = false;
            return Ok(0);
        }

        if self.remaining == 0 {
            // Object fully consumed; verify CRC on the next read.
            self.pending_verify = true;
            return self.read(buf);
        }

        let to_read = std::cmp::min(buf.len() as u64, self.remaining) as usize;
        let n = self.file.read(&mut buf[..to_read])?;
        if n == 0 && self.remaining > 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "payload ended before all bytes were read",
            ));
        }
        self.crc = crc32c::crc32c_append(self.crc, &buf[..n]);
        self.remaining -= n as u64;
        if self.remaining == 0 {
            self.pending_verify = true;
        }
        Ok(n)
    }
}

/// Iterator over records in a volume.
pub struct VolumeRecordIterator<'a> {
    reader: &'a VolumeReader,
    offset: u64,
}

impl<'a> Iterator for VolumeRecordIterator<'a> {
    type Item = Result<(RecordHeader, Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        let file_size = match self.reader.file_size() {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };
        if self.offset >= file_size {
            return None;
        }
        // A header must fit.
        if self.offset + HEADER_SIZE as u64 > file_size {
            return Some(Err(Error::CorruptRecord {
                volume: self.reader.number(),
                offset: self.offset,
                message: "truncated record header at end of volume".into(),
            }));
        }
        match self.reader.read_record(self.offset) {
            Ok((header, id, payload)) => {
                let record_size = header.record_size();
                self.offset += record_size;
                Some(Ok((header, id, payload)))
            }
            Err(e) => {
                // Advance past the header so we don't loop forever; caller
                // should treat this as a torn tail and truncate.
                self.offset += HEADER_SIZE as u64;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    fn tmp_path() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("000000000001.blob");
        (dir, path)
    }

    #[test]
    fn append_and_read_roundtrip() {
        let (_dir, path) = tmp_path();
        let mut writer = VolumeWriter::create(&path, 1).unwrap();
        let mut payload = Cursor::new(b"hello world");
        let (loc, _header) = writer.append_record(b"obj-1", &mut payload).unwrap();
        assert_eq!(loc.volume_number, 1);
        assert_eq!(loc.offset, 0);
        assert!(loc.record_size > 0);
        assert_eq!(loc.record_size % 8, 0);

        let reader = VolumeReader::open(&path, 1).unwrap();
        let (header, id, payload) = reader.read_record(loc.offset).unwrap();
        assert_eq!(id.as_ref(), b"obj-1");
        assert_eq!(payload.as_ref(), b"hello world");
        assert_eq!(header.payload_len, 11);

        // Verify streaming reader.
        let mut stream = BlobPayloadReader::open(&path, loc.offset, &header, b"obj-1").unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"hello world");
    }

    #[test]
    fn multiple_records_iterable() {
        let (_dir, path) = tmp_path();
        let mut writer = VolumeWriter::create(&path, 1).unwrap();
        for i in 0..5u8 {
            let id = vec![b'k', i];
            let payload = vec![i; 100];
            let (_loc, _header) = writer.append_record(&id, &mut Cursor::new(&payload)).unwrap();
        }

        let reader = VolumeReader::open(&path, 1).unwrap();
        let mut count = 0;
        for rec in reader.iter() {
            let (_header, id, payload) = rec.unwrap();
            assert_eq!(id.len(), 2);
            assert_eq!(payload.len(), 100);
            count += 1;
        }
        assert_eq!(count, 5);
    }

    #[test]
    fn empty_payload_roundtrips() {
        let (_dir, path) = tmp_path();
        let mut writer = VolumeWriter::create(&path, 1).unwrap();
        let (loc, _header) = writer.append_record(b"empty", &mut Cursor::new(&[] as &[u8])).unwrap();
        let reader = VolumeReader::open(&path, 1).unwrap();
        let (header, id, payload) = reader.read_record(loc.offset).unwrap();
        assert_eq!(id.as_ref(), b"empty");
        assert!(payload.is_empty());
        assert_eq!(header.payload_len, 0);
    }
}
