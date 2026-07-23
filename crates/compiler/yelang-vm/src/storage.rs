//! Storage backends for the Yelang VM.
//!
//! The VM executes query scans (`QueryScan`) and link traversals
//! (`QueryTraverse`) against a [`StorageBackend`]. The backend abstracts
//! over where table rows physically live: an in-memory map for tests and
//! lightweight execution, or a real storage engine in production.
//!
//! Tables are identified by a `u64` id (derived from the table's `DefId`).
//! Rows are [`Value`]s — typically `Value::Struct` records whose fields are
//! the table's columns.

use std::collections::HashMap;

use yelang_interner::Symbol;

use crate::value::Value;

/// A storage backend the VM can scan tables from.
///
/// Implementations must be cheap to call repeatedly: the VM may scan the
/// same table many times during a single query (e.g. nested-loop joins and
/// traversals).
pub trait StorageBackend {
    /// Scan a table and return all rows as Values.
    ///
    /// Returns an empty vector if the table is unknown.
    fn scan_table(&self, table_id: u64) -> Vec<Value>;

    /// Get a table's column names.
    ///
    /// Returns an empty vector if the table is unknown.
    fn table_columns(&self, table_id: u64) -> Vec<Symbol>;
}

/// An in-memory storage backend.
///
/// Stores tables as `HashMap<u64, Vec<Value>>`, keyed by table id. Useful
/// for tests, examples, and small datasets that fit in memory.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStorage {
    /// Table rows, keyed by table id.
    tables: HashMap<u64, Vec<Value>>,
    /// Column names per table, keyed by table id.
    columns: HashMap<u64, Vec<Symbol>>,
}

impl InMemoryStorage {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or replace) a table with its column names and rows.
    pub fn insert_table(&mut self, table_id: u64, columns: Vec<Symbol>, rows: Vec<Value>) {
        self.tables.insert(table_id, rows);
        self.columns.insert(table_id, columns);
    }

    /// Insert (or replace) just the rows for a table, leaving any existing
    /// column metadata untouched.
    pub fn insert_rows(&mut self, table_id: u64, rows: Vec<Value>) {
        self.tables.insert(table_id, rows);
    }

    /// Whether a table is present in the store.
    pub fn contains_table(&self, table_id: u64) -> bool {
        self.tables.contains_key(&table_id)
    }
}

impl StorageBackend for InMemoryStorage {
    fn scan_table(&self, table_id: u64) -> Vec<Value> {
        self.tables.get(&table_id).cloned().unwrap_or_default()
    }

    fn table_columns(&self, table_id: u64) -> Vec<Symbol> {
        self.columns.get(&table_id).cloned().unwrap_or_default()
    }
}

/// A storage backend with no tables.
///
/// Every scan returns an empty result. This is the default backend for a
/// freshly created [`Vm`](crate::vm::Vm) so that query execution is always
/// well-defined even before a real backend is attached.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyStorage;

impl StorageBackend for EmptyStorage {
    fn scan_table(&self, _table_id: u64) -> Vec<Value> {
        Vec::new()
    }

    fn table_columns(&self, _table_id: u64) -> Vec<Symbol> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// SimulatedTableStorage
// ---------------------------------------------------------------------------

/// A simulated table storage engine for testing.
///
/// Extends [`InMemoryStorage`] with **filter pushdown**: callers can
/// supply a predicate closure so that only matching rows are returned,
/// simulating a storage engine that evaluates filters internally
/// (e.g. an LSM-tree with a bloom-filter / predicate-pushdown layer).
///
/// # Example
///
/// ```ignore
/// let mut storage = SimulatedTableStorage::new();
/// storage.insert_table(1, columns, rows);
///
/// // Full scan.
/// let all = storage.scan_table(1);
///
/// // Filtered scan (pushdown).
/// let adults = storage.scan_table_filtered(1, |row| {
///     row.get_field(age_sym).and_then(|v| v.as_int()).map_or(false, |a| a >= 18)
/// });
/// ```
#[derive(Debug, Clone, Default)]
pub struct SimulatedTableStorage {
    /// Underlying row data.
    inner: InMemoryStorage,
}

impl SimulatedTableStorage {
    /// Create an empty simulated store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a table with its column names and rows.
    pub fn insert_table(&mut self, table_id: u64, columns: Vec<Symbol>, rows: Vec<Value>) {
        self.inner.insert_table(table_id, columns, rows);
    }

    /// Whether a table is present.
    pub fn contains_table(&self, table_id: u64) -> bool {
        self.inner.contains_table(table_id)
    }

    /// Scan with **filter pushdown**: return only rows for which
    /// `predicate` returns `true`.
    ///
    /// This simulates a storage engine that evaluates the filter
    /// internally rather than returning all rows and filtering in
    /// the execution layer.
    pub fn scan_table_filtered<F>(&self, table_id: u64, predicate: F) -> Vec<Value>
    where
        F: Fn(&Value) -> bool,
    {
        self.inner
            .scan_table(table_id)
            .into_iter()
            .filter(|row| predicate(row))
            .collect()
    }

    /// Number of rows stored for a table (0 if unknown).
    pub fn row_count(&self, table_id: u64) -> usize {
        self.inner.scan_table(table_id).len()
    }
}

impl StorageBackend for SimulatedTableStorage {
    fn scan_table(&self, table_id: u64) -> Vec<Value> {
        self.inner.scan_table(table_id)
    }

    fn table_columns(&self, table_id: u64) -> Vec<Symbol> {
        self.inner.table_columns(table_id)
    }
}

// ---------------------------------------------------------------------------
// DistributedSimStorage
// ---------------------------------------------------------------------------

/// A simulated distributed storage engine for testing.
///
/// Data is split across multiple **shards**, each backed by a
/// [`SimulatedTableStorage`]. Scans merge results from every shard,
/// simulating a coordinator that fans out to all nodes and
/// concatenates the responses.
///
/// The [`exchange`](DistributedSimStorage::exchange) method simulates
/// an Exchange operation: it redistributes rows across shards by a
/// hash of the chosen key column, mimicking a network shuffle.
///
/// # Example
///
/// ```ignore
/// let mut dist = DistributedSimStorage::new(3); // 3 shards
/// dist.insert_table(1, columns, rows);         // auto-distributes
/// let all = dist.scan_table(1);                // merges all shards
/// ```
#[derive(Debug, Clone)]
pub struct DistributedSimStorage {
    /// One [`SimulatedTableStorage`] per shard.
    shards: Vec<SimulatedTableStorage>,
}

impl DistributedSimStorage {
    /// Create a distributed store with `num_shards` empty shards.
    ///
    /// # Panics
    ///
    /// Panics if `num_shards` is 0.
    pub fn new(num_shards: usize) -> Self {
        assert!(num_shards > 0, "need at least one shard");
        Self {
            shards: (0..num_shards)
                .map(|_| SimulatedTableStorage::new())
                .collect(),
        }
    }

    /// Number of shards.
    pub fn num_shards(&self) -> usize {
        self.shards.len()
    }

    /// Access a specific shard (for inspection in tests).
    pub fn shard(&self, index: usize) -> Option<&SimulatedTableStorage> {
        self.shards.get(index)
    }

    /// Register a table, distributing rows round-robin across shards.
    pub fn insert_table(&mut self, table_id: u64, columns: Vec<Symbol>, rows: Vec<Value>) {
        // Distribute rows round-robin.
        let mut shard_rows: Vec<Vec<Value>> = vec![Vec::new(); self.shards.len()];
        for (i, row) in rows.into_iter().enumerate() {
            shard_rows[i % self.shards.len()].push(row);
        }
        for (shard, rows) in self.shards.iter_mut().zip(shard_rows) {
            shard.insert_table(table_id, columns.clone(), rows);
        }
    }

    /// Insert rows into a specific shard directly.
    pub fn insert_into_shard(
        &mut self,
        shard_index: usize,
        table_id: u64,
        columns: Vec<Symbol>,
        rows: Vec<Value>,
    ) {
        if let Some(shard) = self.shards.get_mut(shard_index) {
            shard.insert_table(table_id, columns, rows);
        }
    }

    /// Scan with filter pushdown across all shards, merging results.
    pub fn scan_table_filtered<F>(&self, table_id: u64, predicate: F) -> Vec<Value>
    where
        F: Fn(&Value) -> bool,
    {
        self.shards
            .iter()
            .flat_map(|shard| shard.scan_table_filtered(table_id, &predicate))
            .collect()
    }

    /// Simulate an **Exchange** operation: redistribute all rows of a
    /// table across shards by hashing the value of `key_column`.
    ///
    /// This mimics a network shuffle where rows are routed to the
    /// shard responsible for their key range.
    pub fn exchange(&mut self, table_id: u64, key_column: Symbol) {
        // Gather all rows and columns.
        let all_rows = self.scan_table(table_id);
        let columns = self.table_columns(table_id);

        // Clear existing data.
        for shard in &mut self.shards {
            shard.insert_table(table_id, columns.clone(), Vec::new());
        }

        // Redistribute by hash of key column.
        let mut shard_rows: Vec<Vec<Value>> = vec![Vec::new(); self.shards.len()];
        for row in all_rows {
            let hash = row
                .get_field(key_column)
                .map(|v| value_hash(v))
                .unwrap_or(0);
            let target = hash % self.shards.len();
            shard_rows[target].push(row);
        }
        for (shard, rows) in self.shards.iter_mut().zip(shard_rows) {
            shard.insert_table(table_id, columns.clone(), rows);
        }
    }

    /// Total row count across all shards for a table.
    pub fn total_row_count(&self, table_id: u64) -> usize {
        self.shards.iter().map(|s| s.row_count(table_id)).sum()
    }
}

impl StorageBackend for DistributedSimStorage {
    /// Scan merges results from **all** shards.
    fn scan_table(&self, table_id: u64) -> Vec<Value> {
        self.shards
            .iter()
            .flat_map(|shard| shard.scan_table(table_id))
            .collect()
    }

    /// Column metadata is taken from the first shard that knows the table.
    fn table_columns(&self, table_id: u64) -> Vec<Symbol> {
        self.shards
            .iter()
            .find(|s| s.contains_table(table_id))
            .map(|s| s.table_columns(table_id))
            .unwrap_or_default()
    }
}

/// Simple deterministic hash for a [`Value`], used for exchange
/// partitioning. Not cryptographic — just needs to be stable and
/// reasonably distributed.
fn value_hash(v: &Value) -> usize {
    match v {
        Value::Int(i) => (*i as usize).wrapping_mul(2654435761),
        Value::Uint(u) => (*u as usize).wrapping_mul(2654435761),
        Value::Str(s) => {
            // Use the symbol's raw value as a hash seed.
            s.as_usize().wrapping_mul(2654435761)
        }
        Value::Bool(b) => usize::from(*b),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_columns(interner: &yelang_interner::Interner) -> (Symbol, Symbol) {
        (interner.intern("id"), interner.intern("age"))
    }

    fn make_row(interner: &yelang_interner::Interner, id: i128, age: i128) -> Value {
        let (id_sym, age_sym) = test_columns(interner);
        Value::Struct(
            1,
            vec![
                (id_sym, Value::Int(id)),
                (age_sym, Value::Int(age)),
            ],
        )
    }

    #[test]
    fn simulated_table_basic_scan() {
        let interner = yelang_interner::Interner::new();
        let (id_sym, age_sym) = test_columns(&interner);

        let mut storage = SimulatedTableStorage::new();
        storage.insert_table(
            1,
            vec![id_sym, age_sym],
            vec![
                make_row(&interner, 1, 25),
                make_row(&interner, 2, 17),
                make_row(&interner, 3, 30),
            ],
        );

        assert_eq!(storage.scan_table(1).len(), 3);
        assert_eq!(storage.table_columns(1).len(), 2);
        assert!(storage.contains_table(1));
        assert!(!storage.contains_table(99));
    }

    #[test]
    fn simulated_table_filter_pushdown() {
        let interner = yelang_interner::Interner::new();
        let (id_sym, age_sym) = test_columns(&interner);

        let mut storage = SimulatedTableStorage::new();
        storage.insert_table(
            1,
            vec![id_sym, age_sym],
            vec![
                make_row(&interner, 1, 25),
                make_row(&interner, 2, 17),
                make_row(&interner, 3, 30),
            ],
        );

        // Filter: age >= 18.
        let adults = storage.scan_table_filtered(1, |row| {
            row.get_field(age_sym)
                .and_then(|v| v.as_int())
                .map_or(false, |a| a >= 18)
        });
        assert_eq!(adults.len(), 2, "expected 2 adults, got {}", adults.len());
    }

    #[test]
    fn distributed_scan_merges_shards() {
        let interner = yelang_interner::Interner::new();
        let (id_sym, age_sym) = test_columns(&interner);

        let mut dist = DistributedSimStorage::new(3);
        dist.insert_table(
            1,
            vec![id_sym, age_sym],
            vec![
                make_row(&interner, 1, 20),
                make_row(&interner, 2, 30),
                make_row(&interner, 3, 40),
                make_row(&interner, 4, 50),
                make_row(&interner, 5, 60),
            ],
        );

        // All 5 rows should come back from the merged scan.
        assert_eq!(dist.scan_table(1).len(), 5);
        assert_eq!(dist.total_row_count(1), 5);
        assert_eq!(dist.num_shards(), 3);
    }

    #[test]
    fn distributed_filter_pushdown() {
        let interner = yelang_interner::Interner::new();
        let (id_sym, age_sym) = test_columns(&interner);

        let mut dist = DistributedSimStorage::new(2);
        dist.insert_table(
            1,
            vec![id_sym, age_sym],
            vec![
                make_row(&interner, 1, 10),
                make_row(&interner, 2, 20),
                make_row(&interner, 3, 30),
            ],
        );

        let result = dist.scan_table_filtered(1, |row| {
            row.get_field(age_sym)
                .and_then(|v| v.as_int())
                .map_or(false, |a| a >= 20)
        });
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn distributed_exchange_redistributes() {
        let interner = yelang_interner::Interner::new();
        let (id_sym, age_sym) = test_columns(&interner);

        let mut dist = DistributedSimStorage::new(3);
        dist.insert_table(
            1,
            vec![id_sym, age_sym],
            vec![
                make_row(&interner, 1, 20),
                make_row(&interner, 2, 30),
                make_row(&interner, 3, 40),
            ],
        );

        // Exchange by "id" column.
        dist.exchange(1, id_sym);

        // Total rows should be preserved.
        assert_eq!(dist.total_row_count(1), 3);
        // Scan still returns all rows.
        assert_eq!(dist.scan_table(1).len(), 3);
    }
}
