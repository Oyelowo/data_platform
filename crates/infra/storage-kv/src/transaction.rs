//! LSM transactions.

use bytes::Bytes;

use crate::cursor::LsmCursor;
use crate::engine::LsmEngineInner;
use crate::{Error, Result, SequenceNumber};

/// A write transaction for the LSM engine.
pub struct LsmTransaction {
    pub(crate) inner: LsmEngineInner,
    pub(crate) read_only: bool,
    pub(crate) sequence: SequenceNumber,
    pub(crate) finished: bool,
}

impl LsmTransaction {
    pub fn new(inner: LsmEngineInner, read_only: bool, sequence: SequenceNumber) -> Self {
        Self {
            inner,
            read_only,
            sequence,
            finished: false,
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
}

impl storage_traits::Transaction for LsmTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.inner.get(key, self.sequence)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_write()?;
        self.inner.write(key, value, self.sequence)
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.check_write()?;
        self.inner.delete(key, self.sequence)
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl storage_traits::Cursor<Error = Self::Error>> {
        Ok(LsmCursor::new(
            self.inner.clone(),
            start.map(|s| s.to_vec()),
            end.map(|e| e.to_vec()),
            self.sequence,
        ))
    }

    fn commit(mut self) -> Result<()> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        self.finished = true;
        self.inner.sync()
    }

    fn rollback(mut self) -> Result<()> {
        self.finished = true;
        Ok(())
    }

    fn set_isolation(&mut self, level: storage_traits::IsolationLevel) -> Result<()> {
        match level {
            storage_traits::IsolationLevel::ReadCommitted => Ok(()),
            _ => Err(Error::InvalidArgument("unsupported isolation level".into())),
        }
    }
}
