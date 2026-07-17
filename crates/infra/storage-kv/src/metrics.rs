//! Atomic metrics for the LSM engine.
//!
//! All counters are lock-free `AtomicU64` values.  They are sampled with
//! `Ordering::Relaxed` because they are advisory statistics; correctness does
//! not depend on exact ordering between readers and writers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counters and gauges collected by the engine.
///
/// The structure is intentionally flat: every field is an atomic counter that
/// can be incremented from the hot read/write paths without allocation.
#[derive(Default, Debug)]
pub struct Metrics {
    // Compression
    /// Uncompressed bytes fed to compression.
    pub compression_bytes_in: AtomicU64,
    /// Compressed (or stored uncompressed) bytes produced by compression.
    pub compression_bytes_out: AtomicU64,
    /// Number of blocks processed by the compression path.
    pub compression_blocks: AtomicU64,

    // Block cache
    /// Hot-tier cache hits (decompressed block served without I/O).
    pub cache_hot_hits: AtomicU64,
    /// Hot-tier cache misses.
    pub cache_hot_misses: AtomicU64,
    /// Cold-tier cache hits (stored bytes served without disk I/O).
    pub cache_cold_hits: AtomicU64,
    /// Cold-tier cache misses.
    pub cache_cold_misses: AtomicU64,
    /// Block reads that reached disk.
    pub cache_disk_reads: AtomicU64,
    /// Sum of disk-read latencies in microseconds.
    pub cache_disk_read_us_sum: AtomicU64,
    /// Count of disk-read latency samples.
    pub cache_disk_read_us_count: AtomicU64,

    // Compaction
    /// Bytes read from input SSTables during compaction.
    pub compaction_bytes_read: AtomicU64,
    /// Bytes written to output SSTables during compaction.
    pub compaction_bytes_written: AtomicU64,
    /// Input SSTables read during compaction.
    pub compaction_files_read: AtomicU64,
    /// Output SSTables written during compaction.
    pub compaction_files_written: AtomicU64,

    // Blob store
    /// Total bytes stored in blob files.
    pub blob_bytes_total: AtomicU64,
    /// Dead bytes in blob files awaiting reclamation.
    pub blob_bytes_garbage: AtomicU64,
    /// Blob files scanned by GC.
    pub blob_gc_scanned_files: AtomicU64,
    /// Blob records rewritten by GC.
    pub blob_gc_rewritten_records: AtomicU64,
    /// Blob bytes rewritten by GC.
    pub blob_gc_rewritten_bytes: AtomicU64,
    /// Dead blob records discovered by GC.
    pub blob_gc_dead_records: AtomicU64,
    /// Dead blob bytes discovered by GC.
    pub blob_gc_dead_bytes: AtomicU64,
    /// Blob files deleted by GC.
    pub blob_gc_deleted_files: AtomicU64,
    /// Bytes reclaimed by deleting blob files.
    pub blob_gc_space_reclaimed: AtomicU64,
    /// Blob records rewritten by compaction (compaction-integrated GC).
    pub blob_compaction_rewritten_records: AtomicU64,
    /// Blob bytes rewritten by compaction (compaction-integrated GC).
    pub blob_compaction_rewritten_bytes: AtomicU64,
}

impl Metrics {
    /// Record a compression pass.
    pub fn record_compression(&self, bytes_in: u64, bytes_out: u64) {
        self.compression_bytes_in
            .fetch_add(bytes_in, Ordering::Relaxed);
        self.compression_bytes_out
            .fetch_add(bytes_out, Ordering::Relaxed);
        self.compression_blocks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a hot-tier cache hit.
    pub fn record_hot_hit(&self) {
        self.cache_hot_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a hot-tier cache miss.
    pub fn record_hot_miss(&self) {
        self.cache_hot_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cold-tier cache hit.
    pub fn record_cold_hit(&self) {
        self.cache_cold_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cold-tier cache miss.
    pub fn record_cold_miss(&self) {
        self.cache_cold_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a disk block read and its latency.
    pub fn record_disk_read(&self, latency: std::time::Duration) {
        self.cache_disk_reads.fetch_add(1, Ordering::Relaxed);
        let us = latency.as_micros() as u64;
        self.cache_disk_read_us_sum.fetch_add(us, Ordering::Relaxed);
        self.cache_disk_read_us_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record compaction I/O.
    pub fn record_compaction(
        &self,
        bytes_read: u64,
        bytes_written: u64,
        files_read: u64,
        files_written: u64,
    ) {
        self.compaction_bytes_read
            .fetch_add(bytes_read, Ordering::Relaxed);
        self.compaction_bytes_written
            .fetch_add(bytes_written, Ordering::Relaxed);
        self.compaction_files_read
            .fetch_add(files_read, Ordering::Relaxed);
        self.compaction_files_written
            .fetch_add(files_written, Ordering::Relaxed);
    }

    /// Record bytes added to the blob log by a foreground write.
    pub fn record_blob_put(&self, bytes: u64) {
        self.blob_bytes_total.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record dead bytes discovered in a blob file during GC classification.
    pub fn record_blob_garbage(&self, bytes: u64) {
        self.blob_bytes_garbage.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record garbage bytes that were reclaimed (because the file was deleted
    /// or reclassified).
    pub fn record_blob_garbage_reclaimed(&self, bytes: u64) {
        self.blob_bytes_garbage.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Record a blob file deletion and the bytes it frees.
    pub fn record_blob_deleted(&self, total_bytes: u64, garbage_bytes: u64) {
        self.blob_bytes_total
            .fetch_sub(total_bytes, Ordering::Relaxed);
        self.blob_bytes_garbage
            .fetch_sub(garbage_bytes, Ordering::Relaxed);
        self.blob_gc_deleted_files.fetch_add(1, Ordering::Relaxed);
        self.blob_gc_space_reclaimed
            .fetch_add(garbage_bytes, Ordering::Relaxed);
    }

    /// Record a blob reference rewritten during compaction.
    pub fn record_blob_compaction_rewrite(&self, bytes: u64) {
        self.blob_compaction_rewritten_records
            .fetch_add(1, Ordering::Relaxed);
        self.blob_compaction_rewritten_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record statistics from a completed blob GC pass.
    pub fn record_blob_gc_pass(&self, stats: &crate::blob::GcStats) {
        self.blob_gc_scanned_files
            .fetch_add(stats.scanned_files, Ordering::Relaxed);
        self.blob_gc_rewritten_records
            .fetch_add(stats.rewritten_records, Ordering::Relaxed);
        self.blob_gc_rewritten_bytes
            .fetch_add(stats.rewritten_bytes, Ordering::Relaxed);
        self.blob_gc_dead_records
            .fetch_add(stats.dead_records, Ordering::Relaxed);
        self.blob_gc_dead_bytes
            .fetch_add(stats.dead_bytes, Ordering::Relaxed);
    }

    /// Return a named snapshot of all metrics.
    pub fn snapshot(&self) -> HashMap<String, u64> {
        let mut out = HashMap::new();
        let load = |a: &AtomicU64| a.load(Ordering::Relaxed);
        out.insert(
            "compression_bytes_in".into(),
            load(&self.compression_bytes_in),
        );
        out.insert(
            "compression_bytes_out".into(),
            load(&self.compression_bytes_out),
        );
        out.insert("compression_blocks".into(), load(&self.compression_blocks));
        out.insert("cache_hot_hits".into(), load(&self.cache_hot_hits));
        out.insert("cache_hot_misses".into(), load(&self.cache_hot_misses));
        out.insert("cache_cold_hits".into(), load(&self.cache_cold_hits));
        out.insert("cache_cold_misses".into(), load(&self.cache_cold_misses));
        out.insert("cache_disk_reads".into(), load(&self.cache_disk_reads));
        out.insert(
            "cache_disk_read_us_sum".into(),
            load(&self.cache_disk_read_us_sum),
        );
        out.insert(
            "cache_disk_read_us_count".into(),
            load(&self.cache_disk_read_us_count),
        );
        out.insert(
            "compaction_bytes_read".into(),
            load(&self.compaction_bytes_read),
        );
        out.insert(
            "compaction_bytes_written".into(),
            load(&self.compaction_bytes_written),
        );
        out.insert(
            "compaction_files_read".into(),
            load(&self.compaction_files_read),
        );
        out.insert(
            "compaction_files_written".into(),
            load(&self.compaction_files_written),
        );
        out.insert("blob_bytes_total".into(), load(&self.blob_bytes_total));
        out.insert("blob_bytes_garbage".into(), load(&self.blob_bytes_garbage));
        out.insert(
            "blob_gc_scanned_files".into(),
            load(&self.blob_gc_scanned_files),
        );
        out.insert(
            "blob_gc_rewritten_records".into(),
            load(&self.blob_gc_rewritten_records),
        );
        out.insert(
            "blob_gc_rewritten_bytes".into(),
            load(&self.blob_gc_rewritten_bytes),
        );
        out.insert(
            "blob_gc_dead_records".into(),
            load(&self.blob_gc_dead_records),
        );
        out.insert("blob_gc_dead_bytes".into(), load(&self.blob_gc_dead_bytes));
        out.insert(
            "blob_gc_deleted_files".into(),
            load(&self.blob_gc_deleted_files),
        );
        out.insert(
            "blob_gc_space_reclaimed".into(),
            load(&self.blob_gc_space_reclaimed),
        );
        out.insert(
            "blob_compaction_rewritten_records".into(),
            load(&self.blob_compaction_rewritten_records),
        );
        out.insert(
            "blob_compaction_rewritten_bytes".into(),
            load(&self.blob_compaction_rewritten_bytes),
        );
        out
    }
}
