//! `ColumnarEngineImpl` implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use arrow::array::{
    ArrayRef, BinaryBuilder, BooleanBuilder, Float64Builder, Int64Builder, StringBuilder,
    TimestampMicrosecondBuilder,
};
use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use storage_traits::{ColumnBatch, ColumnarEngine, Predicate, ScanResult};
use storage_wal::{Durability, Wal, WalOptions};

use crate::compaction::{self, CompactionInput};
use crate::manifest::Manifest;
use crate::manifest_wal::{self, ManifestRecord};
use crate::partition::partition_key;
use crate::pin::PinSet;
use crate::reader;
use crate::schema::TableSchema;
use crate::snapshot;
use crate::types::ColumnType;
use crate::writer;
use crate::{ColumnarOptions, Error, Result};

const DEFAULT_PARTITION: &str = "__default";
const WAL_DIR: &str = "manifest-wal";
const TMP_DIR: &str = "tmp";
const SNAPSHOT_DIR: &str = "manifest-snapshot";

/// Alias for a per-partition ingest batch to keep complex signatures readable.
type PartitionBatch = Vec<(String, Vec<Option<Bytes>>)>;

/// Arrow/Parquet-backed analytical columnar engine.
#[derive(Debug)]
pub struct ColumnarEngineImpl {
    path: PathBuf,
    options: ColumnarOptions,
    ingest_lock: Mutex<()>,
    wal: Arc<Wal>,
    manifest: Arc<RwLock<Arc<Manifest>>>,
    next_file_id: AtomicU64,
    file_reads: AtomicU64,
    pins: PinSet,
    wal_next_lsn: AtomicU64,
}

impl ColumnarEngineImpl {
    /// Open or create a columnar engine at `path`.
    pub fn open(path: impl AsRef<Path>, options: ColumnarOptions) -> Result<Self> {
        options.validate()?;
        let path = path.as_ref().to_path_buf();

        std::fs::create_dir_all(&path)?;
        std::fs::create_dir_all(path.join(WAL_DIR))?;
        std::fs::create_dir_all(path.join(TMP_DIR))?;
        std::fs::create_dir_all(path.join(SNAPSHOT_DIR))?;
        std::fs::create_dir_all(path.join(DEFAULT_PARTITION))?;

        let wal_dir = path.join(WAL_DIR);
        let wal = Arc::new(Wal::open(&wal_dir, WalOptions::default())?);

        let (manifest, snapshot_lsn, wal_next_lsn) = recover(&path, &wal)?;
        let next_file_id = AtomicU64::new(manifest.files.len() as u64);

        Ok(Self {
            path,
            options,
            ingest_lock: Mutex::new(()),
            wal,
            manifest: Arc::new(RwLock::new(Arc::new(manifest))),
            next_file_id,
            file_reads: AtomicU64::new(0),
            pins: PinSet::new(),
            wal_next_lsn: AtomicU64::new(wal_next_lsn.max(snapshot_lsn)),
        })
    }

    /// Return the path to the table directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the number of Parquet files opened by scans on this engine.
    pub fn file_reads(&self) -> u64 {
        self.file_reads.load(Ordering::SeqCst)
    }

    /// Replace the table schema. Persisted to the WAL.
    pub fn set_schema(&self, schema: TableSchema) -> Result<()> {
        let _guard = self.ingest_lock.lock();
        let json = serde_json::to_string(&schema)?;
        let record = ManifestRecord::SetSchema { schema_json: json };
        self.append_record(record)?;

        let mut manifest = (**self.manifest.read()).clone();
        manifest.schema = schema;
        *self.manifest.write() = Arc::new(manifest);
        Ok(())
    }

    /// Flush any buffered state to stable storage.
    pub fn sync(&self) -> Result<()> {
        let _guard = self.ingest_lock.lock();
        // The underlying storage_wal fsyncs on every Immediate append, and
        // Parquet files are fsynced before rename, so there is no additional
        // buffered state to flush in this slice.
        Ok(())
    }

    /// Return the number of live files in the manifest.
    pub fn file_count(&self) -> usize {
        self.manifest.read().files.len()
    }

    /// Force a compaction of small files in `partition`.
    ///
    /// If `partition` is `None`, every partition is compacted independently.
    /// Returns the number of input files that were rewritten.
    pub fn force_compaction(&self, partition: Option<&str>) -> Result<usize> {
        let _guard = self.ingest_lock.lock();
        let manifest = {
            let guard = self.manifest.read();
            Arc::clone(&*guard)
        };

        let inputs = compaction::plan(&manifest, &self.options, partition)?;
        if inputs.is_empty() {
            return Ok(0);
        }

        let mut total_removed = 0usize;
        for input in inputs {
            let removed = self.compact_one_partition(input)?;
            total_removed += removed;
        }

        Ok(total_removed)
    }

    fn compact_one_partition(&self, input: CompactionInput) -> Result<usize> {
        let partition = input.partition.clone();
        let removed_paths: Vec<PathBuf> = input.files.iter().map(|f| f.path.clone()).collect();
        let removed_count = removed_paths.len();

        // Read selected files and concatenate into a single RecordBatch using the
        // current schema. Phase 2 compaction does not sort; it simply concatenates.
        let batches = reader::read_files_for_compaction(
            &input.files,
            &self.current_schema(),
            &self.file_reads,
        )?;
        let combined = arrow::compute::concat_batches(&self.current_schema().to_arrow(), &batches)?;

        // Write the combined batch into one or more output files. For simplicity a
        // single output file is produced per compaction invocation in Phase 2.
        let file_id = self.next_file_id.fetch_add(1, Ordering::SeqCst);
        let file_name = format!("{:016x}.parquet", file_id);
        let temp_path = self.path.join(TMP_DIR).join(format!("{}.tmp", file_name));
        let final_path = self.path.join(&partition).join(&file_name);
        std::fs::create_dir_all(final_path.parent().unwrap())?;

        let file_meta = writer::write_batch(
            &temp_path,
            &final_path,
            &partition,
            &self.options,
            &self.current_schema(),
            &combined,
        )?;

        // Atomic swap: WAL record first, then in-memory manifest.
        let record = ManifestRecord::Compact {
            add: vec![file_meta],
            remove: removed_paths.clone(),
        };
        self.append_record(record.clone())?;

        {
            let mut manifest = (**self.manifest.read()).clone();
            manifest_wal::apply_record(&mut manifest, record)?;
            *self.manifest.write() = Arc::new(manifest);
        }

        // Queue the obsolete files for deletion once no scan can reference them.
        self.pins.retire_files(removed_paths);
        self.pins.reap(&self.path)?;

        Ok(removed_count)
    }

    /// Return the current schema.
    fn current_schema(&self) -> TableSchema {
        self.manifest.read().schema.clone()
    }

    /// Take a manifest snapshot and truncate old WAL segments.
    ///
    /// Snapshots are normally written automatically after a configurable number
    /// of WAL records; this method allows callers to force a snapshot.
    pub fn snapshot(&self) -> Result<u64> {
        let _guard = self.ingest_lock.lock();
        let manifest = {
            let guard = self.manifest.read();
            Arc::clone(&*guard)
        };
        // The snapshot contains all records up to but not including
        // `wal_next_lsn`; recovery replays from that byte offset.
        let next_lsn = self.wal_next_lsn.load(Ordering::SeqCst);
        snapshot::write(&self.path, &manifest, next_lsn)?;
        self.wal.truncate_before(next_lsn)?;
        Ok(next_lsn)
    }

    /// Maybe write a snapshot if the WAL has grown enough.
    fn maybe_snapshot(&self) -> Result<()> {
        const BYTES_PER_SNAPSHOT: u64 = 4 * 1024;
        let next_lsn = self.wal_next_lsn.load(Ordering::SeqCst);
        if next_lsn > 0 && next_lsn.is_multiple_of(BYTES_PER_SNAPSHOT) {
            self.snapshot()?;
        }
        Ok(())
    }

    /// Append a manifest record to the WAL and update `wal_next_lsn`.
    fn append_record(&self, record: ManifestRecord) -> Result<()> {
        let encoded = record.encode();
        let lsn = self.wal.append(&encoded, Durability::Immediate)?;
        let next_lsn = lsn + storage_wal::RECORD_HEADER_SIZE as u64 + encoded.len() as u64;
        self.wal_next_lsn.store(next_lsn, Ordering::SeqCst);
        Ok(())
    }
}

impl ColumnarEngine for ColumnarEngineImpl {
    type Error = Error;

    fn ingest(&self, columns: ColumnBatch) -> Result<()> {
        let _guard = self.ingest_lock.lock();

        if columns.is_empty() {
            return Ok(());
        }

        // Capture the current manifest and validate/evolve the schema.
        let mut manifest = (**self.manifest.read()).clone();
        let (new_schema, row_count) = if manifest.schema.columns.is_empty() {
            // First ingest without an explicit schema: infer everything as Utf8.
            let names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
            let schema = TableSchema::infer_from_names(&names);
            let row_count = columns[0].1.len();
            (schema, row_count)
        } else {
            manifest.schema.validate_or_evolve(&columns)?
        };

        let schema_changed = new_schema != manifest.schema;
        manifest.schema = new_schema.clone();

        // Persist a schema change before the file addition.
        if schema_changed {
            let json = serde_json::to_string(&manifest.schema)?;
            let record = ManifestRecord::SetSchema { schema_json: json };
            self.append_record(record)?;
            *self.manifest.write() = Arc::new(manifest.clone());
        }

        // Split the batch by partition if configured.
        let partition_col = self.options.partition_column.clone();
        let groups = split_by_partition(&columns, row_count, partition_col.as_deref())?;

        for (partition, group_columns) in groups {
            let batch = build_record_batch(&new_schema, &group_columns, group_columns[0].1.len())?;

            // Write the batch to a temp Parquet file, then atomically install it.
            let file_id = self.next_file_id.fetch_add(1, Ordering::SeqCst);
            let file_name = format!("{:016x}.parquet", file_id);
            let temp_path = self.path.join(TMP_DIR).join(format!("{}.tmp", file_name));
            let final_path = self.path.join(&partition).join(&file_name);
            std::fs::create_dir_all(final_path.parent().unwrap())?;

            let file_meta = writer::write_batch(
                &temp_path,
                &final_path,
                &partition,
                &self.options,
                &new_schema,
                &batch,
            )?;

            // Append an AddFile WAL record and update the in-memory manifest.
            let record = ManifestRecord::AddFile {
                file_meta: file_meta.clone(),
            };
            self.append_record(record)?;
            manifest.files.push(file_meta);
            *self.manifest.write() = Arc::new(manifest.clone());
        }

        // The ingest lock must be released before any operation that itself
        // acquires the ingest lock (snapshot or compaction) to avoid a reentrant
        // deadlock.
        drop(_guard);
        self.maybe_snapshot()?;

        if self.options.background_compaction {
            let _ = self.force_compaction(None);
        }

        Ok(())
    }

    fn scan(&self, projection: &[&str], predicate: &Predicate) -> Result<ScanResult> {
        let manifest: Arc<Manifest> = {
            let guard = self.manifest.read();
            Arc::clone(&*guard)
        };
        let _pin = self.pins.pin(&manifest);
        reader::scan(
            &manifest,
            projection,
            predicate,
            &self.file_reads,
            self.options.partition_column.as_deref(),
        )
    }
}

/// Split an ingest batch into per-partition batches.
///
/// If no partition column is configured, the entire batch lands in
/// `DEFAULT_PARTITION`. Otherwise each row is routed to a partition derived
/// from the configured column's value. Null partition values are routed to a
/// special `__null` partition so that they remain queryable.
fn split_by_partition(
    columns: &[(String, Vec<Option<Bytes>>)],
    _row_count: usize,
    partition_column: Option<&str>,
) -> Result<Vec<(String, PartitionBatch)>> {
    let partition_column = match partition_column {
        Some(c) => c,
        None => {
            return Ok(vec![(DEFAULT_PARTITION.into(), columns.to_vec())]);
        }
    };

    let partition_values: Vec<Option<Bytes>> = columns
        .iter()
        .find(|(n, _)| n == partition_column)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| {
            Error::Batch(format!(
                "partition column '{partition_column}' is missing from ingest batch"
            ))
        })?;

    let mut buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (row_idx, value) in partition_values.iter().enumerate() {
        let key = partition_key(value.as_ref());
        buckets.entry(key).or_default().push(row_idx);
    }

    let mut result = Vec::with_capacity(buckets.len());
    for (partition, row_indices) in buckets {
        let group_columns: PartitionBatch = columns
            .iter()
            .map(|(name, values)| {
                let selected: Vec<Option<Bytes>> =
                    row_indices.iter().map(|&i| values[i].clone()).collect();
                (name.clone(), selected)
            })
            .collect();
        result.push((partition, group_columns));
    }

    Ok(result)
}

fn build_record_batch(
    schema: &TableSchema,
    columns: &[(String, Vec<Option<Bytes>>)],
    row_count: usize,
) -> Result<RecordBatch> {
    let input_map: HashMap<String, &Vec<Option<Bytes>>> =
        columns.iter().map(|(n, v)| (n.clone(), v)).collect();
    let mut arrays: Vec<(String, ArrayRef)> = Vec::with_capacity(schema.columns.len());

    for def in &schema.columns {
        let values = input_map.get(&def.name);
        let array: ArrayRef = match def.ty {
            ColumnType::Bool => Arc::new(build_boolean_array(values, row_count)?),
            ColumnType::Int64 => Arc::new(build_int64_array(values, row_count)?),
            ColumnType::Float64 => Arc::new(build_float64_array(values, row_count)?),
            ColumnType::Utf8 => Arc::new(build_utf8_array(values, row_count)?),
            ColumnType::Binary => Arc::new(build_binary_array(values, row_count)?),
            ColumnType::TimestampMicros => {
                Arc::new(build_timestamp_micros_array(values, row_count)?)
            }
        };
        arrays.push((def.name.clone(), array));
    }

    RecordBatch::try_from_iter(arrays).map_err(|e| e.into())
}

fn build_boolean_array(
    values: Option<&&Vec<Option<Bytes>>>,
    row_count: usize,
) -> Result<arrow::array::BooleanArray> {
    let mut builder = BooleanBuilder::with_capacity(row_count);
    if let Some(vals) = values {
        for v in vals.iter() {
            match v {
                None => builder.append_null(),
                Some(b) => builder.append_value(parse_bool(b)?),
            }
        }
    } else {
        for _ in 0..row_count {
            builder.append_null();
        }
    }
    Ok(builder.finish())
}

fn build_int64_array(
    values: Option<&&Vec<Option<Bytes>>>,
    row_count: usize,
) -> Result<arrow::array::Int64Array> {
    let mut builder = Int64Builder::with_capacity(row_count);
    if let Some(vals) = values {
        for v in vals.iter() {
            match v {
                None => builder.append_null(),
                Some(b) => builder.append_value(parse_i64(b)?),
            }
        }
    } else {
        for _ in 0..row_count {
            builder.append_null();
        }
    }
    Ok(builder.finish())
}

fn build_float64_array(
    values: Option<&&Vec<Option<Bytes>>>,
    row_count: usize,
) -> Result<arrow::array::Float64Array> {
    let mut builder = Float64Builder::with_capacity(row_count);
    if let Some(vals) = values {
        for v in vals.iter() {
            match v {
                None => builder.append_null(),
                Some(b) => builder.append_value(parse_f64(b)?),
            }
        }
    } else {
        for _ in 0..row_count {
            builder.append_null();
        }
    }
    Ok(builder.finish())
}

fn build_utf8_array(
    values: Option<&&Vec<Option<Bytes>>>,
    row_count: usize,
) -> Result<arrow::array::StringArray> {
    let mut builder = StringBuilder::with_capacity(row_count, 1024);
    if let Some(vals) = values {
        for v in vals.iter() {
            match v {
                None => builder.append_null(),
                Some(b) => {
                    let s = std::str::from_utf8(b)
                        .map_err(|e| Error::Batch(format!("invalid utf8 for Utf8 column: {e}")))?;
                    builder.append_value(s);
                }
            }
        }
    } else {
        for _ in 0..row_count {
            builder.append_null();
        }
    }
    Ok(builder.finish())
}

fn build_binary_array(
    values: Option<&&Vec<Option<Bytes>>>,
    row_count: usize,
) -> Result<arrow::array::BinaryArray> {
    let mut builder = BinaryBuilder::with_capacity(row_count, 1024);
    if let Some(vals) = values {
        for v in vals.iter() {
            match v {
                None => builder.append_null(),
                Some(b) => builder.append_value(b),
            }
        }
    } else {
        for _ in 0..row_count {
            builder.append_null();
        }
    }
    Ok(builder.finish())
}

fn build_timestamp_micros_array(
    values: Option<&&Vec<Option<Bytes>>>,
    row_count: usize,
) -> Result<arrow::array::TimestampMicrosecondArray> {
    let mut builder = TimestampMicrosecondBuilder::with_capacity(row_count);
    if let Some(vals) = values {
        for v in vals.iter() {
            match v {
                None => builder.append_null(),
                Some(b) => builder.append_value(parse_i64(b)?),
            }
        }
    } else {
        for _ in 0..row_count {
            builder.append_null();
        }
    }
    Ok(builder.finish())
}

fn parse_bool(bytes: &Bytes) -> Result<bool> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| Error::Batch(format!("invalid utf8 for Bool value: {e}")))?;
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(Error::Batch(format!("invalid Bool value: {s}"))),
    }
}

fn parse_i64(bytes: &Bytes) -> Result<i64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| Error::Batch(format!("invalid utf8 for Int64 value: {e}")))?;
    s.parse::<i64>()
        .map_err(|e| Error::Batch(format!("invalid Int64 value '{s}': {e}")))
}

fn parse_f64(bytes: &Bytes) -> Result<f64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| Error::Batch(format!("invalid utf8 for Float64 value: {e}")))?;
    s.parse::<f64>()
        .map_err(|e| Error::Batch(format!("invalid Float64 value '{s}': {e}")))
}

fn recover(path: &Path, wal: &Wal) -> Result<(Manifest, u64, u64)> {
    // Attempt to load a snapshot first.
    let (mut manifest, snapshot_lsn) = match snapshot::load(path) {
        Ok((m, lsn)) => (m, lsn),
        Err(_) => (Manifest::empty(), 0u64),
    };

    // Replay the WAL from the snapshot LSN (records with LSN >= snapshot_lsn).
    let mut next_lsn = snapshot_lsn;
    for record in wal.iter(snapshot_lsn)? {
        let record = record?;
        // WAL LSN is a byte offset; the next record starts after this one.
        next_lsn =
            record.lsn + storage_wal::RECORD_HEADER_SIZE as u64 + record.payload.len() as u64;
        let decoded = manifest_wal::ManifestRecord::decode(&record.payload)?;
        manifest_wal::apply_record(&mut manifest, decoded)?;
    }

    // Validate every referenced Parquet file and recompute nothing; a readable
    // footer is sufficient for recovery.
    for file in &manifest.files {
        validate_parquet_file(&file.path)?;
    }

    // Clean up any stale temp files.
    clean_tmp_files(path)?;

    Ok((manifest, snapshot_lsn, next_lsn))
}

fn validate_parquet_file(path: &Path) -> Result<()> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    // Force a full footer read; if this succeeds the file is considered valid.
    let _ = builder.metadata();
    Ok(())
}

fn clean_tmp_files(path: &Path) -> Result<()> {
    let tmp_dir = path.join(TMP_DIR);
    if let Ok(entries) = std::fs::read_dir(&tmp_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".parquet.tmp") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}
