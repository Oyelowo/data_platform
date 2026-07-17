//! Bw-Tree transactions.

use bytes::Bytes;

use crate::cursor::BwTreeCursor;
use crate::engine::EngineInner;
use crate::error::{Error, Result};
use crate::page::Pid;

#[derive(Clone, Debug)]
pub(crate) enum WriteOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// A transaction against a Bw-Tree engine.
pub struct BwTreeTransaction {
    pub(crate) inner: std::sync::Arc<EngineInner>,
    pub(crate) read_only: bool,
    pub(crate) finished: bool,
    pub(crate) snapshot_root: Pid,
    pub(crate) ops: Vec<WriteOp>,
}

impl BwTreeTransaction {
    pub(crate) fn new(
        inner: std::sync::Arc<EngineInner>,
        read_only: bool,
        snapshot_root: Pid,
    ) -> Self {
        Self {
            inner,
            read_only,
            finished: false,
            snapshot_root,
            ops: Vec::new(),
        }
    }

    fn check_write(&self) -> Result<()> {
        if self.read_only {
            return Err(Error::ReadOnlyTxn);
        }
        if self.finished {
            return Err(Error::TxnFinished);
        }
        Ok(())
    }

    fn local_get(&self, key: &[u8]) -> Option<Option<Bytes>> {
        let mut result: Option<Option<Bytes>> = None;
        for op in &self.ops {
            match op {
                WriteOp::Put { key: k, value } if k.as_slice() == key => {
                    result = Some(Some(Bytes::copy_from_slice(value)));
                }
                WriteOp::Delete { key: k } if k.as_slice() == key => {
                    result = Some(None);
                }
                _ => {}
            }
        }
        result
    }
}

impl storage_traits::Transaction for BwTreeTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        if let Some(local) = self.local_get(key) {
            return Ok(local);
        }
        self.inner.tree.get(self.snapshot_root, key)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        });
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Delete { key: key.to_vec() });
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl storage_traits::Cursor<Error = Self::Error>> {
        BwTreeCursor::new(
            std::sync::Arc::clone(&self.inner),
            self.snapshot_root,
            start.map(Bytes::copy_from_slice),
            end.map(Bytes::copy_from_slice),
        )
    }

    fn commit(mut self) -> Result<()> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        self.finished = true;
        self.inner.apply_ops(&self.ops)
    }

    fn rollback(mut self) -> Result<()> {
        self.finished = true;
        self.ops.clear();
        Ok(())
    }

    fn set_isolation(&mut self, level: storage_traits::IsolationLevel) -> Result<()> {
        match level {
            storage_traits::IsolationLevel::ReadCommitted => Ok(()),
            _ => Err(Error::Unsupported("only ReadCommitted is supported")),
        }
    }
}
