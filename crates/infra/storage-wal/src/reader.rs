//! WAL recovery and random-access readers.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bytes::{Buf, Bytes};

use crate::record::{Record, RECORD_HEADER_SIZE};
use crate::segment::{list_segments, read_segment, segment_path};
use crate::{Error, Lsn, Result};

/// Iterate over records in the WAL in ascending LSN order.
pub struct WalIterator {
    dir: PathBuf,
    segments: Vec<Lsn>,
    segment_idx: usize,
    buf: Bytes,
    current_lsn: Lsn,
}

impl WalIterator {
    /// Create an iterator over all records in `dir` starting from `start_lsn`.
    pub fn new(dir: &Path, start_lsn: Lsn) -> Result<Self> {
        let segments = list_segments(dir)?;
        let mut iter = Self {
            dir: dir.to_path_buf(),
            segments,
            segment_idx: 0,
            buf: Bytes::new(),
            current_lsn: start_lsn,
        };
        iter.advance_to_segment(start_lsn)?;
        Ok(iter)
    }

    fn advance_to_segment(&mut self, lsn: Lsn) -> Result<()> {
        self.segment_idx = self
            .segments
            .partition_point(|&first_lsn| first_lsn <= lsn)
            .saturating_sub(1);
        if self.segment_idx >= self.segments.len() {
            return Ok(());
        }
        let first_lsn = self.segments[self.segment_idx];
        let mut data = read_segment(&self.dir, first_lsn)?;
        let offset = (lsn - first_lsn) as usize;
        if offset > data.len() {
            data.clear();
        } else {
            data.drain(..offset);
        }
        self.buf = Bytes::from(data);
        self.current_lsn = lsn;
        Ok(())
    }

    fn load_next_segment(&mut self) -> Result<bool> {
        self.segment_idx += 1;
        if self.segment_idx >= self.segments.len() {
            return Ok(false);
        }
        let first_lsn = self.segments[self.segment_idx];
        self.buf = Bytes::from(read_segment(&self.dir, first_lsn)?);
        self.current_lsn = first_lsn;
        Ok(true)
    }
}

impl Iterator for WalIterator {
    type Item = Result<Record>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.buf.is_empty() {
                match self.load_next_segment() {
                    Ok(true) => {}
                    Ok(false) => return None,
                    Err(e) => return Some(Err(e)),
                }
                if self.buf.is_empty() {
                    return None;
                }
            }

            match Record::decode(&self.buf) {
                Ok(Some((record, consumed))) => {
                    self.buf.advance(consumed);
                    self.current_lsn += consumed as u64;
                    return Some(Ok(record));
                }
                Ok(None) => {
                    // Partial record at end of segment or torn write; stop.
                    if !self.load_next_segment().unwrap_or(false) {
                        return None;
                    }
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

/// Random-access reader for a single WAL record by LSN.
pub struct WalReader {
    dir: PathBuf,
}

impl WalReader {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// Read the record at `lsn`, if any.
    pub fn read(&self, lsn: Lsn) -> Result<Option<Record>> {
        let segments = list_segments(&self.dir)?;
        let idx = segments.partition_point(|&first_lsn| first_lsn <= lsn);
        if idx == 0 {
            return Err(Error::RecordNotFound { lsn });
        }
        let first_lsn = segments[idx - 1];
        let path = segment_path(&self.dir, first_lsn);
        let mut file = File::open(&path)?;
        let offset = (lsn - first_lsn) as u64;
        file.seek(SeekFrom::Start(offset))?;

        // Read enough bytes for the header plus a reasonable payload. We do
        // not know the payload length until we parse the header.
        let mut header = [0u8; RECORD_HEADER_SIZE];
        match file.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        }

        let mut cursor = &header[..];
        let _ = cursor.get_u32_le(); // magic
        let _ = cursor.get_u8(); // type
        let _ = cursor.get_u64_le(); // lsn
        let payload_len = cursor.get_u32_le() as usize;
        let _ = cursor.get_u32_le(); // crc placeholder

        let mut body = vec![0u8; RECORD_HEADER_SIZE + payload_len];
        body[..RECORD_HEADER_SIZE].copy_from_slice(&header);
        file.read_exact(&mut body[RECORD_HEADER_SIZE..])?;

        let (record, _) = Record::decode(&body)?.ok_or_else(|| Error::CorruptRecord {
            lsn,
            reason: "record disappeared during read".into(),
        })?;
        Ok(Some(record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::RecordType;

    fn write_segments(dir: &Path, records: &[Record]) -> Vec<Lsn> {
        use crate::segment::Segment;
        let mut segment = Segment::open(dir, 0, 64 * 1024 * 1024).unwrap();
        let mut lsns = Vec::new();
        let mut lsn = 0u64;
        for mut rec in records.iter().cloned() {
            rec.lsn = lsn;
            let mut buf = Vec::new();
            rec.encode(&mut buf).unwrap();
            segment.append(&buf).unwrap();
            lsns.push(lsn);
            lsn += buf.len() as u64;
        }
        segment.sync().unwrap();
        lsns
    }

    #[test]
    fn iterator_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let records = vec![
            Record::new(RecordType::Put, &b"one"[..]),
            Record::new(RecordType::Put, &b"two"[..]),
            Record::new(RecordType::Delete, &b"three"[..]),
        ];
        write_segments(dir.path(), &records);

        let mut iter = WalIterator::new(dir.path(), 0).unwrap();
        let rec = iter.next().unwrap().unwrap();
        assert_eq!(rec.payload, &b"one"[..]);
        let rec = iter.next().unwrap().unwrap();
        assert_eq!(rec.payload, &b"two"[..]);
        let rec = iter.next().unwrap().unwrap();
        assert_eq!(rec.ty, RecordType::Delete);
        assert!(iter.next().is_none());
    }

    #[test]
    fn reader_by_lsn() {
        let dir = tempfile::tempdir().unwrap();
        let records = vec![
            Record::new(RecordType::Put, &b"alpha"[..]),
            Record::new(RecordType::Put, &b"beta"[..]),
        ];
        let lsns = write_segments(dir.path(), &records);

        let reader = WalReader::new(dir.path());
        let rec = reader.read(lsns[1]).unwrap().unwrap();
        assert_eq!(rec.payload, &b"beta"[..]);
    }
}
