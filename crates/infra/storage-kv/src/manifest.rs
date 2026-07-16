//! MANIFEST log for atomic version edits.
//!
//! Each record is stored as a framed line:
//!
//! ```text
//! <hex-crc32c-8> <json-payload>\n
//! ```
//!
//! The checksum lets recovery detect torn tails and corruption.  A malformed
//! final line is treated as a torn tail and ignored; any other bad line is
//! reported as a fatal manifest error.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::sstable::format::checksum;
use crate::version::FileMetaData;
use crate::version_set::VersionEdit;
use crate::{FileNumber, Result};

/// Manifest records are line-delimited JSON for debuggability. Production
/// engines often use a binary format; JSON keeps this implementation simple
/// and inspectable without sacrificing atomicity.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ManifestRecord {
    cf_id: u32,
    deleted_files: Vec<(usize, FileNumber)>,
    new_files: Vec<ManifestFileMeta>,
    next_file_number: FileNumber,
    last_sequence: u64,
    created_cfs: Vec<(u32, String)>,
    dropped_cfs: Vec<u32>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ManifestFileMeta {
    level: usize,
    number: FileNumber,
    file_size: u64,
    smallest: Vec<u8>,
    largest: Vec<u8>,
}

pub struct Manifest {
    file: File,
    #[allow(dead_code)]
    path: PathBuf,
}

impl Manifest {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(&path)?;
        Ok(Self { file, path })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new().append(true).read(true).open(&path)?;
        Ok(Self { file, path })
    }

    pub fn log_edit(&mut self, edit: &VersionEdit) -> Result<()> {
        let record = ManifestRecord {
            cf_id: edit.cf_id,
            deleted_files: edit.deleted_files.clone(),
            new_files: edit
                .new_files
                .iter()
                .map(|(level, m)| ManifestFileMeta {
                    level: *level,
                    number: m.number,
                    file_size: m.file_size,
                    smallest: m.smallest.clone(),
                    largest: m.largest.clone(),
                })
                .collect(),
            next_file_number: edit.next_file_number,
            last_sequence: edit.last_sequence,
            created_cfs: edit.created_cfs.clone(),
            dropped_cfs: edit.dropped_cfs.clone(),
        };
        let json = serde_json::to_vec(&record)?;
        let crc = checksum(&json);
        self.file.write_all(format!("{crc:08x} ").as_bytes())?;
        self.file.write_all(&json)?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn recover(path: impl AsRef<Path>) -> Result<Vec<VersionEdit>> {
        let path = path.as_ref();
        let mut file = File::open(path)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;

        let lines: Vec<&str> = buf.lines().collect();
        let last_idx = lines.len().saturating_sub(1);
        let mut edits = Vec::new();
        for (idx, line) in lines.into_iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            match parse_manifest_line(line) {
                Ok(edit) => edits.push(edit),
                Err((malformed, e)) => {
                    if idx == last_idx && malformed {
                        // Torn tail on the final line: stop recovery here.
                        break;
                    }
                    return Err(e);
                }
            }
        }
        Ok(edits)
    }
}

/// Parse a single manifest line.
///
/// On error the returned boolean is `true` if the line is structurally
/// malformed (i.e. could be a torn tail) and `false` if the structure is
/// valid but the checksum does not match (real corruption).
fn parse_manifest_line(line: &str) -> std::result::Result<VersionEdit, (bool, crate::Error)> {
    let (checksum_hex, json) = line.split_once(' ').ok_or_else(|| {
        (
            true,
            crate::Error::Manifest("missing checksum delimiter".into()),
        )
    })?;
    if checksum_hex.len() != 8 {
        return Err((true, crate::Error::Manifest("bad checksum length".into())));
    }
    let expected = u32::from_str_radix(checksum_hex, 16).map_err(|e| {
        (
            true,
            crate::Error::Manifest(format!("invalid checksum hex: {e}")),
        )
    })?;
    let got = checksum(json.as_bytes());
    if got != expected {
        return Err((
            false,
            crate::Error::Manifest(format!(
                "checksum mismatch: expected {expected:#08x}, got {got:#08x}"
            )),
        ));
    }
    let record: ManifestRecord = serde_json::from_str(json).map_err(|e| {
        (
            true,
            crate::Error::Manifest(format!("bad manifest json: {e}")),
        )
    })?;
    Ok(record.into())
}

impl From<ManifestRecord> for VersionEdit {
    fn from(record: ManifestRecord) -> Self {
        Self {
            cf_id: record.cf_id,
            deleted_files: record.deleted_files,
            new_files: record
                .new_files
                .into_iter()
                .map(|m| {
                    (
                        m.level,
                        FileMetaData {
                            number: m.number,
                            file_size: m.file_size,
                            smallest: m.smallest,
                            largest: m.largest,
                        },
                    )
                })
                .collect(),
            next_file_number: record.next_file_number,
            last_sequence: record.last_sequence,
            created_cfs: record.created_cfs,
            dropped_cfs: record.dropped_cfs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_edit(n: u64) -> VersionEdit {
        VersionEdit {
            cf_id: 0,
            last_sequence: n,
            next_file_number: n + 1,
            deleted_files: vec![],
            new_files: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MANIFEST");
        {
            let mut m = Manifest::create(&path).unwrap();
            m.log_edit(&sample_edit(10)).unwrap();
            m.log_edit(&sample_edit(20)).unwrap();
        }
        let edits = Manifest::recover(&path).unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].last_sequence, 10);
        assert_eq!(edits[1].last_sequence, 20);
    }

    #[test]
    fn manifest_torn_tail_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MANIFEST");
        {
            let mut m = Manifest::create(&path).unwrap();
            m.log_edit(&sample_edit(10)).unwrap();
        }
        // Append garbage that looks like a torn write of a final line.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"dea")
            .unwrap();

        let edits = Manifest::recover(&path).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].last_sequence, 10);
    }

    #[test]
    fn manifest_corrupt_middle_line_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MANIFEST");
        {
            let mut m = Manifest::create(&path).unwrap();
            m.log_edit(&sample_edit(10)).unwrap();
            m.log_edit(&sample_edit(20)).unwrap();
        }
        let contents = std::fs::read_to_string(&path).unwrap();
        // Corrupt the checksum of the first line.
        let mut lines: Vec<&str> = contents.lines().collect();
        lines[0] = "00000000";
        std::fs::write(&path, lines.join("\n")).unwrap();

        assert!(Manifest::recover(&path).is_err());
    }
}
