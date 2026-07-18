//! Transaction identifiers, timestamps, and transaction-state tracking for MVCC.
//!
//! Phase 6 adds multi-record transactions.  Each transaction gets a monotonic
//! `TxnId` at begin time and a monotonic `Timestamp` at commit time.  The
//! transaction table tracks which transactions are active, committed, or
//! aborted so that the version-visibility function can decide whether a version
//! created by a given txn id is visible to a reader.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::sync::Mutex as SyncMutex;

use crate::error::{Error, Result};

/// Logical transaction identifier.  `0` is reserved (no transaction / autocommit).
pub type TxnId = u64;
/// Monotonic timestamp used for snapshot visibility and commit ordering.
pub type Timestamp = u64;

/// Sentinel meaning "no transaction".
pub const NULL_TXN_ID: TxnId = 0;
/// Sentinel meaning "no timestamp".
pub const NULL_TS: Timestamp = 0;

/// Isolation level for a transaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IsolationLevel {
    /// Each statement sees data committed before the statement began.
    ReadCommitted,
    /// The whole transaction sees a single snapshot taken at begin time.
    #[default]
    Snapshot,
}

/// Runtime state of a transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxnState {
    /// Still running; created at the given read timestamp.
    Active { read_ts: Timestamp },
    /// Committed with the given commit timestamp.
    Committed(Timestamp),
    /// Aborted.
    Aborted,
}

/// Oracle used by the version-visibility function.
///
/// The transaction table is the production implementation; the trait exists so
/// unit tests can inject deterministic commit/aborted sets.
pub trait TxnOracle: Send + Sync {
    /// Return the commit timestamp for `txn_id`, or `None` if it is not
    /// committed.
    fn commit_ts(&self, txn_id: TxnId) -> Option<Timestamp>;
    /// True if the transaction is still active.
    fn is_active(&self, txn_id: TxnId) -> bool;
    /// True if the transaction aborted.
    fn is_aborted(&self, txn_id: TxnId) -> bool;
}

/// In-memory transaction table.  It is rebuilt during recovery by scanning
/// `Begin`/`Commit`/`Abort` WAL records.
#[derive(Debug, Default)]
pub struct TransactionTable {
    /// Single logical clock.  `begin` returns odd values, `commit` returns even
    /// values; interleaving them guarantees a total order between txn ids and
    /// commit timestamps.
    clock: AtomicU64,
    /// Active transactions, indexed by `TxnId`.
    active: SyncMutex<HashMap<TxnId, TxnState>>,
    /// Committed transactions: `TxnId -> commit Timestamp`.
    committed: SyncMutex<HashMap<TxnId, Timestamp>>,
    /// Aborted transactions.
    aborted: SyncMutex<HashSet<TxnId>>,
}

impl TransactionTable {
    /// Create an empty transaction table.
    pub fn new() -> Self {
        Self {
            clock: AtomicU64::new(1),
            active: SyncMutex::new(HashMap::new()),
            committed: SyncMutex::new(HashMap::new()),
            aborted: SyncMutex::new(HashSet::new()),
        }
    }

    /// Start a new transaction.  Returns its `TxnId` and `read_ts`.
    pub fn begin(&self, isolation: IsolationLevel) -> Result<(TxnId, Timestamp)> {
        let txn_id = self.clock.fetch_add(2, Ordering::SeqCst);
        if txn_id == NULL_TXN_ID {
            return Err(Error::Corruption("transaction id overflow to NULL".into()));
        }
        let raw_ts = self.current_timestamp();
        let read_ts = read_ts_for(isolation, raw_ts);
        self.active.with_mut(|active| {
            active.insert(txn_id, TxnState::Active { read_ts });
        });
        Ok((txn_id, read_ts))
    }

    /// Return a snapshot read timestamp for a statement or transaction.
    pub fn current_timestamp(&self) -> Timestamp {
        self.clock.load(Ordering::SeqCst)
    }

    /// Reserve the next commit timestamp without updating transaction state.
    ///
    /// Used by callers that must log a `Commit` record before marking the
    /// transaction committed in memory.
    pub fn reserve_commit_ts(&self) -> Timestamp {
        self.clock.fetch_add(2, Ordering::SeqCst)
    }

    /// Commit `txn_id`.  Returns the assigned commit timestamp.
    pub fn commit(&self, txn_id: TxnId) -> Result<Timestamp> {
        if txn_id == NULL_TXN_ID {
            return Err(Error::InvalidArgument(
                "cannot commit the NULL transaction".into(),
            ));
        }
        let commit_ts = self.reserve_commit_ts();
        self.active.with_mut(|active| {
            active.remove(&txn_id);
        });
        self.committed.with_mut(|committed| {
            committed.insert(txn_id, commit_ts);
        });
        Ok(commit_ts)
    }

    /// Mark `txn_id` as aborted.
    pub fn abort(&self, txn_id: TxnId) -> Result<()> {
        if txn_id == NULL_TXN_ID {
            return Err(Error::InvalidArgument(
                "cannot abort the NULL transaction".into(),
            ));
        }
        self.active.with_mut(|active| {
            active.remove(&txn_id);
        });
        self.aborted.with_mut(|aborted| {
            aborted.insert(txn_id);
        });
        Ok(())
    }

    /// Record a transaction as committed with an explicit timestamp.  Used
    /// during recovery when replaying `Commit` records.
    pub fn recover_committed(&self, txn_id: TxnId, commit_ts: Timestamp) -> Result<()> {
        if txn_id == NULL_TXN_ID {
            return Ok(());
        }
        self.active.with_mut(|active| {
            active.remove(&txn_id);
        });
        self.committed.with_mut(|committed| {
            committed.insert(txn_id, commit_ts);
        });
        Ok(())
    }

    /// Return the state of `txn_id`, if known.
    pub fn state(&self, txn_id: TxnId) -> Option<TxnState> {
        if txn_id == NULL_TXN_ID {
            return None;
        }
        if let Some(s) = self.active.with_mut(|active| active.get(&txn_id).copied()) {
            return Some(s);
        }
        if let Some(ts) = self
            .committed
            .with_mut(|committed| committed.get(&txn_id).copied())
        {
            return Some(TxnState::Committed(ts));
        }
        if self.aborted.with_mut(|aborted| aborted.contains(&txn_id)) {
            return Some(TxnState::Aborted);
        }
        None
    }
}

impl TxnOracle for TransactionTable {
    fn commit_ts(&self, txn_id: TxnId) -> Option<Timestamp> {
        if txn_id == NULL_TXN_ID {
            return Some(NULL_TS);
        }
        self.committed
            .with_mut(|committed| committed.get(&txn_id).copied())
    }

    fn is_active(&self, txn_id: TxnId) -> bool {
        txn_id != NULL_TXN_ID && self.active.with_mut(|active| active.contains_key(&txn_id))
    }

    fn is_aborted(&self, txn_id: TxnId) -> bool {
        txn_id != NULL_TXN_ID && self.aborted.with_mut(|aborted| aborted.contains(&txn_id))
    }
}

fn read_ts_for(isolation: IsolationLevel, current_ts: Timestamp) -> Timestamp {
    match isolation {
        // Snapshot sees the world as of just before the transaction began.
        IsolationLevel::Snapshot => current_ts.saturating_sub(1),
        // ReadCommitted re-evaluates every statement; we pass the current
        // timestamp from the caller at statement time.
        IsolationLevel::ReadCommitted => current_ts.saturating_sub(1),
    }
}

/// A lightweight handle passed to tree operations.
#[derive(Clone, Debug)]
pub struct Transaction {
    pub txn_id: TxnId,
    pub read_ts: Timestamp,
    pub isolation: IsolationLevel,
    /// LSN of the most recent WAL record written by this transaction.
    pub last_lsn: Arc<AtomicU64>,
}

impl Transaction {
    /// Create a transaction handle.  The caller is responsible for registering
    /// it in a `TransactionTable`.
    pub fn new(txn_id: TxnId, read_ts: Timestamp, isolation: IsolationLevel) -> Self {
        Self {
            txn_id,
            read_ts,
            isolation,
            last_lsn: Arc::new(AtomicU64::new(crate::wal::NULL_LSN)),
        }
    }

    /// Update the transaction's last LSN.
    pub fn set_last_lsn(&self, lsn: crate::wal::Lsn) {
        self.last_lsn.store(lsn, Ordering::SeqCst);
    }

    /// Return the transaction's last LSN.
    pub fn last_lsn(&self) -> crate::wal::Lsn {
        self.last_lsn.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_commit_abort_lifecycle() {
        let table = TransactionTable::new();
        let (txn_id, read_ts) = table.begin(IsolationLevel::Snapshot).unwrap();
        assert!(txn_id > NULL_TXN_ID);
        assert_eq!(table.state(txn_id), Some(TxnState::Active { read_ts }));

        let commit_ts = table.commit(txn_id).unwrap();
        assert!(commit_ts > read_ts);
        assert_eq!(table.state(txn_id), Some(TxnState::Committed(commit_ts)));

        let (txn_id2, _) = table.begin(IsolationLevel::Snapshot).unwrap();
        table.abort(txn_id2).unwrap();
        assert_eq!(table.state(txn_id2), Some(TxnState::Aborted));
    }

    #[test]
    fn oracle_sees_committed_and_aborted() {
        let table = TransactionTable::new();
        let (t1, _) = table.begin(IsolationLevel::Snapshot).unwrap();
        let commit_ts = table.commit(t1).unwrap();
        assert_eq!(table.commit_ts(t1), Some(commit_ts));
        assert!(!table.is_active(t1));

        let (t2, _) = table.begin(IsolationLevel::Snapshot).unwrap();
        assert!(table.is_active(t2));
        assert_eq!(table.commit_ts(t2), None);

        let (t3, _) = table.begin(IsolationLevel::Snapshot).unwrap();
        table.abort(t3).unwrap();
        assert!(table.is_aborted(t3));
        assert!(!table.is_active(t3));
    }
}
