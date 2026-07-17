//! Scan execution: projection, predicate pushdown, and Arrow-to-bytes conversion.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use arrow::array::{
    Array, ArrayRef, BinaryArray, BinaryBuilder, BooleanArray, BooleanBuilder, Float64Array,
    Float64Builder, Int64Array, Int64Builder, StringArray, StringBuilder,
    TimestampMicrosecondArray, TimestampMicrosecondBuilder,
};
use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use storage_traits::{Predicate, ScanResult};

use crate::Result;
use crate::manifest::{FileMeta, Manifest};
use crate::partition;
use crate::predicate;
use crate::schema::{ColumnDef, TableSchema};
use crate::types::ColumnType;

/// Scan a snapshot of the manifest.
///
/// `file_reads` is incremented once for every Parquet file that is opened.
/// Returns columns in the order requested by `projection`.
pub fn scan(
    manifest: &Manifest,
    projection: &[&str],
    predicate: &Predicate,
    file_reads: &AtomicU64,
    partition_column: Option<&str>,
) -> Result<ScanResult> {
    // Validate projection columns exist in the schema.
    for col in projection {
        if manifest.schema.column(col).is_none() {
            return Err(crate::Error::Schema(format!(
                "projection column '{col}' does not exist in schema"
            )));
        }
    }

    // Determine all columns we need to read: projection + predicate columns.
    let mut needed = projection.iter().copied().collect::<HashSet<&str>>();
    collect_predicate_columns(predicate, &mut needed);

    let mut accumulators: HashMap<String, Vec<Option<Bytes>>> = HashMap::new();
    for col in projection {
        accumulators.insert(col.to_string(), Vec::new());
    }

    for file in &manifest.files {
        // Partition pruning: skip the whole file if its partition directory
        // cannot match the predicate on the partition column. We only prune for
        // Utf8/Binary partition columns because directory names are strings and
        // numeric comparisons on string representations can produce false
        // positives (e.g. "20" > "100"). Stats pruning below handles numeric
        // columns correctly.
        if let Some(part_col) = partition_column
            && manifest.schema.column(part_col).is_some_and(|c| {
                matches!(
                    c.ty,
                    crate::types::ColumnType::Utf8 | crate::types::ColumnType::Binary
                )
            })
            && !partition::partition_prune(part_col, &file.partition, predicate)
        {
            continue;
        }

        if !predicate::prune_file_by_stats(predicate, &file.column_stats, &manifest.schema) {
            continue;
        }
        read_file(
            file,
            &manifest.schema,
            projection,
            &needed,
            predicate,
            &mut accumulators,
            file_reads,
        )?;
    }

    let mut result = Vec::with_capacity(projection.len());
    for col in projection {
        result.push((
            col.to_string(),
            accumulators.remove(*col).unwrap_or_default(),
        ));
    }
    Ok(result)
}

fn collect_predicate_columns<'a>(predicate: &'a Predicate, out: &mut HashSet<&'a str>) {
    match predicate {
        Predicate::True => {}
        Predicate::Eq { column, .. } | Predicate::Range { column, .. } => {
            out.insert(column);
        }
        Predicate::And(children) | Predicate::Or(children) => {
            for c in children {
                collect_predicate_columns(c, out);
            }
        }
    }
}

fn read_file(
    file: &FileMeta,
    schema: &TableSchema,
    projection: &[&str],
    needed: &HashSet<&str>,
    predicate: &Predicate,
    accumulators: &mut HashMap<String, Vec<Option<Bytes>>>,
    file_reads: &AtomicU64,
) -> Result<()> {
    let file_handle = std::fs::File::open(&file.path)?;
    file_reads.fetch_add(1, Ordering::SeqCst);

    let builder = ParquetRecordBatchReaderBuilder::try_new(file_handle)?;
    let file_arrow_schema = builder.schema();

    // Build a list of column names that exist in this file and are needed.
    let file_cols: Vec<String> = file_arrow_schema
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();

    let mut root_indices = Vec::new();
    let mut present_needed = Vec::new();
    for (idx, name) in file_cols.iter().enumerate() {
        if needed.contains(name.as_str()) {
            root_indices.push(idx);
            present_needed.push(name.clone());
        }
    }

    let builder = if root_indices.is_empty() {
        // No needed columns exist in the file (e.g. a pure count(*) with only
        // a never-matching predicate). We still need the row count for
        // filtering, so read a single metadata-only pass.
        builder.with_batch_size(1)
    } else {
        let mask = ProjectionMask::roots(builder.parquet_schema(), root_indices.clone());
        builder.with_projection(mask)
    };

    let reader = builder.build()?;

    for batch in reader {
        let batch = batch?;
        let num_rows = batch.num_rows();

        // Convert every needed column that is present in the file to option bytes.
        let mut column_values: HashMap<String, Vec<Option<Bytes>>> = HashMap::new();
        for (name, array) in batch.schema().fields().iter().zip(batch.columns()) {
            let name = name.name();
            if !needed.contains(name.as_str()) {
                continue;
            }
            let Some(def) = schema.column(name) else {
                continue;
            };
            column_values.insert(name.clone(), array_to_opt_bytes(array, def)?);
        }

        // For needed columns missing from the file, supply all-null vectors.
        for name in needed.iter() {
            if !column_values.contains_key(*name) {
                column_values.insert(name.to_string(), vec![None; num_rows]);
            }
        }

        // Evaluate predicate row-by-row and copy matching rows into accumulators.
        for row in 0..num_rows {
            let mut row_map: HashMap<String, Option<Bytes>> = HashMap::new();
            for name in needed.iter() {
                row_map.insert(
                    name.to_string(),
                    column_values
                        .get(*name)
                        .and_then(|v| v.get(row))
                        .cloned()
                        .flatten(),
                );
            }

            if predicate::eval_row(predicate, schema, &row_map)? {
                for col in projection {
                    let value = column_values
                        .get(*col)
                        .and_then(|v| v.get(row))
                        .cloned()
                        .flatten();
                    accumulators.get_mut(*col).unwrap().push(value);
                }
            }
        }
    }

    Ok(())
}

fn array_to_opt_bytes(array: &dyn Array, def: &ColumnDef) -> Result<Vec<Option<Bytes>>> {
    match def.ty {
        ColumnType::Bool => {
            let arr = array
                .as_any()
                .downcast_ref::<BooleanArray>()
                .ok_or_else(|| crate::Error::Batch("expected BooleanArray".into()))?;
            Ok((0..arr.len())
                .map(|i| {
                    if arr.is_null(i) {
                        None
                    } else if arr.value(i) {
                        Some(Bytes::from_static(b"true"))
                    } else {
                        Some(Bytes::from_static(b"false"))
                    }
                })
                .collect())
        }
        ColumnType::Int64 => {
            let arr = array
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| crate::Error::Batch("expected Int64Array".into()))?;
            Ok((0..arr.len())
                .map(|i| {
                    if arr.is_null(i) {
                        None
                    } else {
                        Some(Bytes::from(arr.value(i).to_string()))
                    }
                })
                .collect())
        }
        ColumnType::Float64 => {
            let arr = array
                .as_any()
                .downcast_ref::<Float64Array>()
                .ok_or_else(|| crate::Error::Batch("expected Float64Array".into()))?;
            Ok((0..arr.len())
                .map(|i| {
                    if arr.is_null(i) {
                        None
                    } else {
                        Some(Bytes::from(arr.value(i).to_string()))
                    }
                })
                .collect())
        }
        ColumnType::Utf8 => {
            let arr = array
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| crate::Error::Batch("expected StringArray".into()))?;
            Ok((0..arr.len())
                .map(|i| {
                    if arr.is_null(i) {
                        None
                    } else {
                        Some(Bytes::copy_from_slice(arr.value(i).as_bytes()))
                    }
                })
                .collect())
        }
        ColumnType::Binary => {
            let arr = array
                .as_any()
                .downcast_ref::<BinaryArray>()
                .ok_or_else(|| crate::Error::Batch("expected BinaryArray".into()))?;
            Ok((0..arr.len())
                .map(|i| {
                    if arr.is_null(i) {
                        None
                    } else {
                        Some(Bytes::copy_from_slice(arr.value(i)))
                    }
                })
                .collect())
        }
        ColumnType::TimestampMicros => {
            let arr = array
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .ok_or_else(|| crate::Error::Batch("expected TimestampMicrosecondArray".into()))?;
            Ok((0..arr.len())
                .map(|i| {
                    if arr.is_null(i) {
                        None
                    } else {
                        Some(Bytes::from(arr.value(i).to_string()))
                    }
                })
                .collect())
        }
    }
}

/// Read all rows from `files` using `schema` as the output schema.
///
/// Missing columns in older files are returned as null. This is used by the
/// compaction pass to produce a single concatenated batch.
pub fn read_files_for_compaction(
    files: &[FileMeta],
    schema: &TableSchema,
    file_reads: &AtomicU64,
) -> Result<Vec<RecordBatch>> {
    let all_column_names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    let mut batches = Vec::with_capacity(files.len());

    for file in files {
        let file_handle = std::fs::File::open(&file.path)?;
        file_reads.fetch_add(1, Ordering::SeqCst);

        let builder = ParquetRecordBatchReaderBuilder::try_new(file_handle)?;
        let file_arrow_schema = builder.schema();
        let file_cols: Vec<String> = file_arrow_schema
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();

        let mut root_indices = Vec::new();
        for (idx, name) in file_cols.iter().enumerate() {
            if all_column_names.contains(&name.as_str()) {
                root_indices.push(idx);
            }
        }

        let builder = if root_indices.is_empty() {
            builder.with_batch_size(1)
        } else {
            let mask = ProjectionMask::roots(builder.parquet_schema(), root_indices);
            builder.with_projection(mask)
        };

        let reader = builder.build()?;
        for batch in reader {
            let batch = batch?;
            let num_rows = batch.num_rows();

            let mut column_values: HashMap<String, Vec<Option<Bytes>>> = HashMap::new();
            for (name, array) in batch.schema().fields().iter().zip(batch.columns()) {
                let name = name.name();
                let Some(def) = schema.column(name) else {
                    continue;
                };
                column_values.insert(name.clone(), array_to_opt_bytes(array, def)?);
            }

            // Build an output batch in the requested schema order.
            let mut arrays: Vec<(String, ArrayRef)> = Vec::with_capacity(schema.columns.len());
            for def in &schema.columns {
                let values = column_values
                    .get(&def.name)
                    .cloned()
                    .unwrap_or_else(|| vec![None; num_rows]);
                let array: ArrayRef = match def.ty {
                    ColumnType::Bool => Arc::new(build_boolean_array(&values)?),
                    ColumnType::Int64 => Arc::new(build_int64_array(&values)?),
                    ColumnType::Float64 => Arc::new(build_float64_array(&values)?),
                    ColumnType::Utf8 => Arc::new(build_utf8_array(&values)?),
                    ColumnType::Binary => Arc::new(build_binary_array(&values)?),
                    ColumnType::TimestampMicros => Arc::new(build_timestamp_micros_array(&values)?),
                };
                arrays.push((def.name.clone(), array));
            }

            batches.push(RecordBatch::try_from_iter(arrays)?);
        }
    }

    Ok(batches)
}

fn build_boolean_array(values: &[Option<Bytes>]) -> Result<arrow::array::BooleanArray> {
    let mut builder = BooleanBuilder::with_capacity(values.len());
    for v in values {
        match v {
            None => builder.append_null(),
            Some(b) => builder.append_value(parse_bool(b)?),
        }
    }
    Ok(builder.finish())
}

fn build_int64_array(values: &[Option<Bytes>]) -> Result<arrow::array::Int64Array> {
    let mut builder = Int64Builder::with_capacity(values.len());
    for v in values {
        match v {
            None => builder.append_null(),
            Some(b) => builder.append_value(parse_i64(b)?),
        }
    }
    Ok(builder.finish())
}

fn build_float64_array(values: &[Option<Bytes>]) -> Result<arrow::array::Float64Array> {
    let mut builder = Float64Builder::with_capacity(values.len());
    for v in values {
        match v {
            None => builder.append_null(),
            Some(b) => builder.append_value(parse_f64(b)?),
        }
    }
    Ok(builder.finish())
}

fn build_utf8_array(values: &[Option<Bytes>]) -> Result<arrow::array::StringArray> {
    let mut builder = StringBuilder::with_capacity(values.len(), 1024);
    for v in values {
        match v {
            None => builder.append_null(),
            Some(b) => {
                let s = std::str::from_utf8(b).map_err(|e| {
                    crate::Error::Batch(format!("invalid utf8 for Utf8 column: {e}"))
                })?;
                builder.append_value(s);
            }
        }
    }
    Ok(builder.finish())
}

fn build_binary_array(values: &[Option<Bytes>]) -> Result<arrow::array::BinaryArray> {
    let mut builder = BinaryBuilder::with_capacity(values.len(), 1024);
    for v in values {
        match v {
            None => builder.append_null(),
            Some(b) => builder.append_value(b),
        }
    }
    Ok(builder.finish())
}

fn build_timestamp_micros_array(
    values: &[Option<Bytes>],
) -> Result<arrow::array::TimestampMicrosecondArray> {
    let mut builder = TimestampMicrosecondBuilder::with_capacity(values.len());
    for v in values {
        match v {
            None => builder.append_null(),
            Some(b) => builder.append_value(parse_i64(b)?),
        }
    }
    Ok(builder.finish())
}

fn parse_bool(bytes: &Bytes) -> Result<bool> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| crate::Error::Batch(format!("invalid utf8 for Bool value: {e}")))?;
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(crate::Error::Batch(format!("invalid Bool value: {s}"))),
    }
}

fn parse_i64(bytes: &Bytes) -> Result<i64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| crate::Error::Batch(format!("invalid utf8 for Int64 value: {e}")))?;
    s.parse::<i64>()
        .map_err(|e| crate::Error::Batch(format!("invalid Int64 value '{s}': {e}")))
}

fn parse_f64(bytes: &Bytes) -> Result<f64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| crate::Error::Batch(format!("invalid utf8 for Float64 value: {e}")))?;
    s.parse::<f64>()
        .map_err(|e| crate::Error::Batch(format!("invalid Float64 value '{s}': {e}")))
}
