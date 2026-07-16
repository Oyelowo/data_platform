//! MANIFEST log for atomic version edits.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::version::FileMetaData;
use crate::version_set::VersionEdit;
use crate::{FileNumber, Result};

/// Manifest records are line-delimited JSON for debuggability. Production
/// engines often use a binary format; JSON keeps this implementation simple
/// and inspectable without sacrificing atomicity.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ManifestRecord {
    deleted_files: Vec<(usize, FileNumber)>,
    new_files: Vec<ManifestFileMeta>,
    next_file_number: FileNumber,
    last_sequence: u64,
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
        };
        let line = serde_json::to_vec(&record)?;
        self.file.write_all(&line)?;
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

        let mut edits = Vec::new();
        for line in buf.lines() {
            if line.is_empty() {
                continue;
            }
            let record: ManifestRecord = serde_json::from_str(line)
                .map_err(|e| crate::Error::Manifest(format!("bad manifest line: {e}")))?;
            let edit = VersionEdit {
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
            };
            edits.push(edit);
        }
        Ok(edits)
    }
}
