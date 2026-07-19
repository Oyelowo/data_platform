//! Helpers for index maintenance operations.

use bytes::Bytes;
use storage_traits::transaction::Transaction;

use crate::catalog::{IndexCatalog, IndexDef};
use crate::keys::{index_key, primary_key};
use crate::record::Record;

/// Insert or update index entries for a primary record inside a transaction.
///
/// `old_record` is the previous value (if any) so that stale entries for the
/// same key can be removed before the new entries are written.
pub fn update_indexes<Txn, E>(
    txn: &mut Txn,
    catalog: &IndexCatalog,
    primary_key: &[u8],
    old_record: Option<&Record>,
    new_record: Option<&Record>,
) -> Result<(), E>
where
    Txn: Transaction<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    // Remove stale entries from active indexes that existed on the old record.
    if let Some(old) = old_record {
        for (_, def) in catalog.active() {
            delete_index_entry(txn, def, primary_key, old)?;
        }
    }

    // Insert new entries from active indexes that exist on the new record.
    if let Some(new) = new_record {
        for (_, def) in catalog.active() {
            insert_index_entry(txn, def, primary_key, new)?;
        }
    }

    Ok(())
}

fn insert_index_entry<Txn, E>(
    txn: &mut Txn,
    def: &IndexDef,
    pk: &[u8],
    record: &Record,
) -> Result<(), E>
where
    Txn: Transaction<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    for col in &def.columns {
        if let Some(value) = record.columns.get(col) {
            let key = index_key(def.id.0, value, pk);
            txn.put(&key, pk)?;
        }
    }
    Ok(())
}

fn delete_index_entry<Txn, E>(
    txn: &mut Txn,
    def: &IndexDef,
    pk: &[u8],
    record: &Record,
) -> Result<(), E>
where
    Txn: Transaction<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    for col in &def.columns {
        if let Some(value) = record.columns.get(col) {
            let key = index_key(def.id.0, value, pk);
            txn.delete(&key)?;
        }
    }
    Ok(())
}

/// Build index entries for a backfill: insert one entry per active index and
/// column present in `record`.
pub fn backfill_record<Txn, E>(
    txn: &mut Txn,
    def: &IndexDef,
    pk: &[u8],
    record: &Record,
) -> Result<(), E>
where
    Txn: Transaction<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    insert_index_entry(txn, def, pk, record)
}

/// Read a record from raw bytes. Returns `None` for opaque (non-record) values.
pub fn maybe_record(value: &Bytes) -> Option<Record> {
    Record::decode(value)
}

/// Delete a primary record and all of its active index entries.
pub fn delete_primary<Txn, E>(
    txn: &mut Txn,
    catalog: &IndexCatalog,
    key: &[u8],
    old_value: Option<Bytes>,
) -> Result<(), E>
where
    Txn: Transaction<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    if let Some(bytes) = old_value
        && let Some(record) = maybe_record(&bytes)
    {
        update_indexes(txn, catalog, key, Some(&record), None)?;
    }
    txn.delete(&primary_key(key))?;
    Ok(())
}
