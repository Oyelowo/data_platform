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
