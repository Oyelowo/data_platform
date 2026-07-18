//! Deterministic fault injection for the WAL committer.
//!
//! `FaultConfig` lets tests simulate `fsync` failures and power loss (lost
//! buffered writes) in a controlled, reproducible way. It is not intended for
//! production use.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Counters used to decide when to inject a fault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FaultConfig {
    /// If `Some(n)`, fail the `n`th `Segment::flush()` call.
    pub fail_flush_every: Option<usize>,
    /// If `Some(n)`, fail the `n`th `Segment::sync()` / `sync_all()` call.
    pub fail_sync_every: Option<usize>,
    /// If true, `write_record` updates the LSN but does not write bytes to the
    /// segment. This simulates the OS page cache losing buffered writes before
    /// they reach stable storage. Use only with `Durability::Buffered` tests;
    /// `Durability::Immediate` would incorrectly report durability.
    pub drop_appends: bool,
}

/// Thread-safe mutable fault-injection state.
#[derive(Debug, Clone)]
pub struct FaultInjector {
    config: Arc<Mutex<FaultConfig>>,
    flush_count: Arc<AtomicUsize>,
    sync_count: Arc<AtomicUsize>,
    dropped_appends: Arc<AtomicBool>,
}

impl FaultInjector {
    /// Create a new injector from a config.
    pub fn new(config: FaultConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            flush_count: Arc::new(AtomicUsize::new(0)),
            sync_count: Arc::new(AtomicUsize::new(0)),
            dropped_appends: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Replace the active config.
    pub fn set_config(&self, config: FaultConfig) {
        let mut guard = self.config.lock().unwrap();
        *guard = config;
    }

    /// Return true if the current `append` should be dropped (power-loss sim).
    pub fn should_drop_append(&self) -> bool {
        let guard = self.config.lock().unwrap();
        if guard.drop_appends {
            self.dropped_appends.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Return true if a `flush` fault should be injected now.
    pub fn should_fail_flush(&self) -> bool {
        let guard = self.config.lock().unwrap();
        if let Some(n) = guard.fail_flush_every {
            let count = self.flush_count.fetch_add(1, Ordering::Relaxed) + 1;
            count == n
        } else {
            false
        }
    }

    /// Return true if a `sync` fault should be injected now.
    pub fn should_fail_sync(&self) -> bool {
        let guard = self.config.lock().unwrap();
        if let Some(n) = guard.fail_sync_every {
            let count = self.sync_count.fetch_add(1, Ordering::Relaxed) + 1;
            count == n
        } else {
            false
        }
    }

    /// True if any append was dropped since the injector was created.
    pub fn dropped_appends(&self) -> bool {
        self.dropped_appends.load(Ordering::Relaxed)
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new(FaultConfig::default())
    }
}
