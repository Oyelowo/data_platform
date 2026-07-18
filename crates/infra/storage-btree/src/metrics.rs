//! Lightweight operational metrics for the B+ tree engine.
//!
//! Counters are best-effort and use `Relaxed` ordering; they do not participate
//! in correctness.  They are intended for observability, diagnostics, and
//! coarse capacity planning.

use std::sync::atomic::{AtomicU64, Ordering};

/// Snapshot of engine metrics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BtreeMetrics {
    /// Number of point gets.
    pub gets: u64,
    /// Number of point puts.
    pub puts: u64,
    /// Number of deletes.
    pub deletes: u64,
    /// Number of range scans started.
    pub scans_started: u64,
    /// Number of transactions begun.
    pub txns_begun: u64,
    /// Number of transactions committed.
    pub txns_committed: u64,
    /// Number of transactions rolled back.
    pub txns_rolled_back: u64,

    /// Buffer-pool frame hits (page already resident).
    pub cache_hits: u64,
    /// Buffer-pool frame misses (page read from disk).
    pub cache_misses: u64,
    /// Number of pages evicted from the buffer pool.
    pub evictions: u64,
    /// Number of dirty pages flushed to disk.
    pub page_flushes: u64,

    /// Bytes appended to the WAL.
    pub wal_bytes_written: u64,
    /// Number of WAL fsync calls.
    pub wal_syncs: u64,
    /// Cumulative WAL fsync latency in nanoseconds.
    pub wal_sync_latency_ns: u64,

    /// Bytes appended to the value log.
    pub value_log_bytes_written: u64,
    /// Number of value-log fsync calls.
    pub value_log_syncs: u64,

    /// Number of checkpoints written.
    pub checkpoints: u64,
}

impl BtreeMetrics {
    /// Create a new zeroed metrics collection.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Internal mutable metrics used by engine components.
#[derive(Debug, Default)]
pub struct Metrics {
    gets: AtomicU64,
    puts: AtomicU64,
    deletes: AtomicU64,
    scans_started: AtomicU64,
    txns_begun: AtomicU64,
    txns_committed: AtomicU64,
    txns_rolled_back: AtomicU64,

    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    evictions: AtomicU64,
    page_flushes: AtomicU64,

    wal_bytes_written: AtomicU64,
    wal_syncs: AtomicU64,
    wal_sync_latency_ns: AtomicU64,

    value_log_bytes_written: AtomicU64,
    value_log_syncs: AtomicU64,

    checkpoints: AtomicU64,
}

impl Metrics {
    /// Create a new zeroed internal metrics collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the point-get counter.
    pub fn inc_gets(&self) {
        self.gets.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the point-put counter.
    pub fn inc_puts(&self) {
        self.puts.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the delete counter.
    pub fn inc_deletes(&self) {
        self.deletes.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the scan-started counter.
    pub fn inc_scans_started(&self) {
        self.scans_started.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the transactions-begun counter.
    pub fn inc_txns_begun(&self) {
        self.txns_begun.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the transactions-committed counter.
    pub fn inc_txns_committed(&self) {
        self.txns_committed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the transactions-rolled-back counter.
    pub fn inc_txns_rolled_back(&self) {
        self.txns_rolled_back.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the cache-hit counter.
    pub fn inc_cache_hits(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the cache-miss counter.
    pub fn inc_cache_misses(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the eviction counter.
    pub fn inc_evictions(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the page-flush counter.
    pub fn inc_page_flushes(&self) {
        self.page_flushes.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the WAL bytes-written counter by `n`.
    pub fn inc_wal_bytes(&self, n: u64) {
        self.wal_bytes_written.fetch_add(n, Ordering::Relaxed);
    }

    /// Increment the WAL sync counter and add `latency_ns` to the total.
    pub fn record_wal_sync(&self, latency_ns: u64) {
        self.wal_syncs.fetch_add(1, Ordering::Relaxed);
        self.wal_sync_latency_ns
            .fetch_add(latency_ns, Ordering::Relaxed);
    }

    /// Increment the value-log bytes-written counter by `n`.
    pub fn inc_value_log_bytes(&self, n: u64) {
        self.value_log_bytes_written.fetch_add(n, Ordering::Relaxed);
    }

    /// Increment the value-log sync counter.
    pub fn inc_value_log_syncs(&self) {
        self.value_log_syncs.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the checkpoint counter.
    pub fn inc_checkpoints(&self) {
        self.checkpoints.fetch_add(1, Ordering::Relaxed);
    }

    /// Take a snapshot of all counters.
    pub fn snapshot(&self) -> BtreeMetrics {
        BtreeMetrics {
            gets: self.gets.load(Ordering::Relaxed),
            puts: self.puts.load(Ordering::Relaxed),
            deletes: self.deletes.load(Ordering::Relaxed),
            scans_started: self.scans_started.load(Ordering::Relaxed),
            txns_begun: self.txns_begun.load(Ordering::Relaxed),
            txns_committed: self.txns_committed.load(Ordering::Relaxed),
            txns_rolled_back: self.txns_rolled_back.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            page_flushes: self.page_flushes.load(Ordering::Relaxed),
            wal_bytes_written: self.wal_bytes_written.load(Ordering::Relaxed),
            wal_syncs: self.wal_syncs.load(Ordering::Relaxed),
            wal_sync_latency_ns: self.wal_sync_latency_ns.load(Ordering::Relaxed),
            value_log_bytes_written: self.value_log_bytes_written.load(Ordering::Relaxed),
            value_log_syncs: self.value_log_syncs.load(Ordering::Relaxed),
            checkpoints: self.checkpoints.load(Ordering::Relaxed),
        }
    }
}
