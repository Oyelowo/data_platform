//! Options for transactions and engine I/O.

/// Isolation level for a transaction.
///
/// Not all engines support all levels. If a level is unsupported, the engine
/// should return [`Error::Unsupported`](crate::Error::Unsupported).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IsolationLevel {
    /// Reads may see uncommitted writes from other transactions.
    ReadUncommitted,
    /// Reads only see committed data.
    #[default]
    ReadCommitted,
    /// Within a transaction, repeated reads see the same snapshot.
    RepeatableRead,
    /// Transactions are fully serializable.
    Serializable,
    /// Multi-version concurrency control snapshot.
    Snapshot,
}

/// Options used when beginning a transaction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TxnOptions {
    /// Whether the transaction is allowed to mutate data.
    pub read_only: bool,
    /// Desired isolation level.
    pub isolation: IsolationLevel,
}

impl TxnOptions {
    /// Return options for a read-only transaction.
    pub fn read_only() -> Self {
        Self {
            read_only: true,
            isolation: IsolationLevel::Snapshot,
        }
    }
}
