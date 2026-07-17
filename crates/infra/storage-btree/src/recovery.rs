//! Recovery logic for the B+ tree engine.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::{Buf, BufMut};
use crc32c::crc32c;

use crate::error::{Error, Result};
use crate::options::BtreeOptions;
use crate::page::NULL_PAGE_ID;
use crate::pager::Pager;
use crate::tree::Tree;
use crate::wal_record::{BatchOp, WalRecord};

/// Name of the metadata file.
const META_FILE: &str = "META";

/// Persistent engine metadata.
#[derive(Clone, Debug)]
pub(crate) struct Meta {
    pub root: crate::page::PageId,
    pub freelist: Vec<crate::page::PageId>,
    pub next_page_id: crate::page::PageId,
}

impl Meta {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"BTRE-META\0");
        buf.extend_from_slice(&self.root.to_le_bytes());
        buf.extend_from_slice(&(self.freelist.len() as u64).to_le_bytes());
        for id in &self.freelist {
            buf.extend_from_slice(&id.to_le_bytes());
        }
        buf.extend_from_slice(&self.next_page_id.to_le_bytes());
        let checksum = crc32c(&buf);
        buf.put_u32_le(checksum);
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 10 + 8 + 8 + 8 + 4 {
            return Err(Error::Corruption("meta file truncated".into()));
        }
        if &data[..10] != b"BTRE-META\0" {
            return Err(Error::Corruption("meta file magic mismatch".into()));
        }
        let (body, checksum_bytes) = data.split_at(data.len() - 4);
        let expected = crc32c(body);
        let mut cv = checksum_bytes;
        let got = cv.get_u32_le();
        if expected != got {
            return Err(Error::Corruption(format!(
                "meta checksum mismatch: expected {expected:#x}, got {got:#x}"
            )));
        }

        let mut view = &body[10..];
        let root = view.get_u64_le();
        let freelist_len = view.get_u64_le() as usize;
        let mut freelist = Vec::with_capacity(freelist_len);
        for _ in 0..freelist_len {
            freelist.push(view.get_u64_le());
        }
        let next_page_id = view.get_u64_le();
        Ok(Self {
            root,
            freelist,
            next_page_id,
        })
    }
}

pub(crate) fn meta_path(dir: &Path) -> PathBuf {
    dir.join(META_FILE)
}

/// Read the metadata file if it exists.
pub(crate) fn read_meta(dir: &Path) -> Result<Option<Meta>> {
    let path = meta_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(&path)?;
    Ok(Some(Meta::decode(&data)?))
}

/// Atomically write the metadata file.
///
/// Durability rules: the temporary file is fsynced before the rename, and the
/// parent directory is fsynced after the rename. This guarantees that the new
/// `META` entry is reachable and on stable storage before we truncate the WAL.
pub(crate) fn write_meta(dir: &Path, meta: &Meta) -> Result<()> {
    let path = meta_path(dir);
    let tmp = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)?;
    let bytes = meta.encode();
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, &path)?;
    let dir_file = File::open(dir)?;
    dir_file.sync_all()?;
    Ok(())
}

/// Replay WAL records into a tree, returning the new root page id.
pub(crate) fn replay_wal(
    wal: &storage_wal::Wal,
    tree: &Tree,
    meta_root: crate::page::PageId,
) -> Result<crate::page::PageId> {
    let mut root = meta_root;
    for record in wal.iter(0)? {
        let record = record?;
        let op = WalRecord::decode(&record.payload)?;
        match op {
            WalRecord::Put { key, value } => {
                root = tree.insert(root, &key, &value)?;
            }
            WalRecord::Delete { key } => {
                root = tree.delete(root, &key)?;
            }
            WalRecord::Batch(ops) => {
                for op in ops {
                    match op {
                        BatchOp::Put { key, value } => {
                            root = tree.insert(root, &key, &value)?;
                        }
                        BatchOp::Delete { key } => {
                            root = tree.delete(root, &key)?;
                        }
                    }
                }
            }
        }
    }
    Ok(root)
}

/// Recover engine state on open using the provided WAL handle.
pub(crate) fn recover(
    dir: &Path,
    options: &BtreeOptions,
    pager: Arc<Pager>,
    wal: &storage_wal::Wal,
) -> Result<crate::page::PageId> {
    let meta = read_meta(dir)?;
    let tree = Tree::new(Arc::clone(&pager), options);
    let meta_root = meta.as_ref().map_or(NULL_PAGE_ID, |m| {
        pager.restore_freelist(m.freelist.clone(), m.next_page_id);
        m.root
    });

    let root = replay_wal(wal, &tree, meta_root)?;

    // If we recovered from WAL, persist the new state and truncate WAL.
    if meta_root != root || wal.iter(0)?.next().is_some() {
        // Reclaim any pages that were retired in a previous run but not yet
        // moved to the freelist. The in-memory `retired` set is not persisted,
        // so replaying the WAL recreates the tree state and lets us identify
        // the currently unreachable pages.
        let reachable = tree.reachable_pages(root)?;
        pager.reclaim_unreachable(&reachable)?;

        let (freelist, next_page_id) = pager.freelist_snapshot();
        let new_meta = Meta {
            root,
            freelist,
            next_page_id,
        };
        // The new pages must be durable before the META checkpoint becomes
        // reachable, otherwise a crash could leave META pointing at torn pages
        // while the (now truncated) WAL can no longer replay them.
        pager.sync()?;
        write_meta(dir, &new_meta)?;
        // Remove completed WAL segments. The active segment is kept open by the
        // committer; truncating it would lose subsequent writes.
        wal.truncate_completed()?;
    }

    Ok(root)
}
