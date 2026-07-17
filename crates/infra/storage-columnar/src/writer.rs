//! Parquet file writer and statistics capture.

use std::fs::File;
use std::path::Path;
use std::time::SystemTime;

use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::{EnabledStatistics, WriterProperties, WriterVersion};

use crate::manifest::{ColumnStats, FileMeta};
use crate::schema::TableSchema;
use crate::types::ColumnType;
use crate::{ColumnarOptions, Result};

/// Write a single `RecordBatch` to a Parquet file.
///
/// The file is written to `temp_path`, fsynced, renamed to `final_path`, and
/// the parent directory is fsynced.
pub fn write_batch(
    temp_path: &Path,
    final_path: &Path,
    partition: &str,
    options: &ColumnarOptions,
    schema: &TableSchema,
    batch: &RecordBatch,
) -> Result<FileMeta> {
    let file = File::create(temp_path)?;
    let props = writer_properties(options);
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))?;
    writer.write(batch)?;
    let metadata = writer.close()?;

    // Reopen the temp file to fsync it before the atomic rename.
    let temp_file = File::open(temp_path)?;
    temp_file.sync_all()?;
    drop(temp_file);

    std::fs::rename(temp_path, final_path)?;
    if let Some(parent) = final_path.parent() {
        sync_dir(parent)?;
    }

    let row_count = metadata.num_rows as usize;
    let column_stats = read_column_stats(final_path, schema)?;

    Ok(FileMeta {
        path: final_path.to_path_buf(),
        partition: partition.into(),
        row_count,
        created_at: SystemTime::now(),
        column_stats,
    })
}

fn writer_properties(options: &ColumnarOptions) -> WriterProperties {
    WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .set_writer_version(WriterVersion::PARQUET_2_0)
        .set_statistics_enabled(EnabledStatistics::Page)
        .set_max_row_group_size(options.row_group_size)
        .set_data_page_size_limit(options.data_page_size)
        .build()
}

fn read_column_stats(
    path: &Path,
    schema: &TableSchema,
) -> Result<std::collections::HashMap<String, ColumnStats>> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let metadata = builder.metadata();
    let mut map = std::collections::HashMap::new();

    for row_group in metadata.row_groups() {
        for col_meta in row_group.columns() {
            let col_path = col_meta.column_path();
            let name = col_path.parts().first().cloned().unwrap_or_default();
            let Some(def) = schema.column(&name) else {
                continue;
            };
            let Some(stats) = col_meta.statistics() else {
                map.entry(name.clone()).or_insert_with(ColumnStats::unknown);
                continue;
            };

            let entry = map.entry(name.clone()).or_insert_with(ColumnStats::unknown);
            entry.null_count += stats.null_count_opt().unwrap_or(0) as usize;

            let (min_bytes, max_bytes) = stats_min_max(stats, def.ty)?;

            if entry.min.is_empty() && entry.max.is_empty() {
                entry.min = min_bytes;
                entry.max = max_bytes;
                continue;
            }

            update_typed_min_max(entry, min_bytes, max_bytes, def.ty);
        }
    }

    Ok(map)
}

/// Update `entry.min`/`entry.max` using typed comparison so that numeric types
/// compare by value rather than by lexicographic byte order.
fn update_typed_min_max(entry: &mut ColumnStats, min: Bytes, max: Bytes, ty: ColumnType) {
    match ty {
        ColumnType::Int64 | ColumnType::TimestampMicros => {
            let entry_min = parse_i64(&entry.min);
            let entry_max = parse_i64(&entry.max);
            let new_min = parse_i64(&min);
            let new_max = parse_i64(&max);
            if let (Ok(cur), Ok(v)) = (entry_min, new_min) {
                if v < cur {
                    entry.min = min;
                }
            } else {
                entry.min = Bytes::new();
            }
            if let (Ok(cur), Ok(v)) = (entry_max, new_max) {
                if v > cur {
                    entry.max = max;
                }
            } else {
                entry.max = Bytes::new();
            }
        }
        ColumnType::Float64 => {
            let entry_min = parse_f64(&entry.min);
            let entry_max = parse_f64(&entry.max);
            let new_min = parse_f64(&min);
            let new_max = parse_f64(&max);
            if let (Ok(cur), Ok(v)) = (entry_min, new_min) {
                if v < cur {
                    entry.min = min;
                }
            } else {
                entry.min = Bytes::new();
            }
            if let (Ok(cur), Ok(v)) = (entry_max, new_max) {
                if v > cur {
                    entry.max = max;
                }
            } else {
                entry.max = Bytes::new();
            }
        }
        ColumnType::Bool | ColumnType::Utf8 | ColumnType::Binary => {
            if min.as_ref() < entry.min.as_ref() {
                entry.min = min;
            }
            if max.as_ref() > entry.max.as_ref() {
                entry.max = max;
            }
        }
    }
}

fn stats_min_max(
    stats: &parquet::file::statistics::Statistics,
    _ty: ColumnType,
) -> Result<(Bytes, Bytes)> {
    use parquet::file::statistics::Statistics;

    let unknown = || (Bytes::new(), Bytes::new());

    match stats {
        Statistics::Boolean(s) => {
            let min = s.min_opt().copied();
            let max = s.max_opt().copied();
            match (min, max) {
                (Some(min), Some(max)) => Ok((bool_to_bytes(min), bool_to_bytes(max))),
                _ => Ok(unknown()),
            }
        }
        Statistics::Int64(s) => {
            let min = s.min_opt().copied();
            let max = s.max_opt().copied();
            match (min, max) {
                (Some(min), Some(max)) => {
                    Ok((Bytes::from(min.to_string()), Bytes::from(max.to_string())))
                }
                _ => Ok(unknown()),
            }
        }
        Statistics::Double(s) => {
            let min = s.min_opt().copied();
            let max = s.max_opt().copied();
            match (min, max) {
                (Some(min), Some(max)) => {
                    Ok((Bytes::from(min.to_string()), Bytes::from(max.to_string())))
                }
                _ => Ok(unknown()),
            }
        }
        Statistics::ByteArray(s) => {
            let min = s.min_opt();
            let max = s.max_opt();
            match (min, max) {
                (Some(min), Some(max)) => Ok((
                    Bytes::copy_from_slice(min.data()),
                    Bytes::copy_from_slice(max.data()),
                )),
                _ => Ok(unknown()),
            }
        }
        _ => Ok(unknown()),
    }
}

fn bool_to_bytes(value: bool) -> Bytes {
    Bytes::from_static(if value { b"true" } else { b"false" })
}

fn parse_i64(bytes: &Bytes) -> Result<i64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| crate::Error::Batch(format!("invalid utf8 for i64: {e}")))?;
    s.parse::<i64>()
        .map_err(|e| crate::Error::Batch(format!("invalid i64 '{s}': {e}")))
}

fn parse_f64(bytes: &Bytes) -> Result<f64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| crate::Error::Batch(format!("invalid utf8 for f64: {e}")))?;
    s.parse::<f64>()
        .map_err(|e| crate::Error::Batch(format!("invalid f64 '{s}': {e}")))
}

fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let dir = File::open(path)?;
        dir.sync_all()?;
    }
    Ok(())
}
