//! Metrics exposed through `Engine::stats()`.

use bytes::Bytes;
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 512,
        level0_file_num_compaction_trigger: 2,
        level0_slowdown_writes_trigger: 128,
        level0_stop_writes_trigger: 256,
        target_file_size_base: 256,
        max_bytes_for_level_base: 256,
        block_cache_size: 64 * 1024,
        compressed_block_cache_size: 1024 * 1024,
        ..Default::default()
    }
}

fn get_metric(stats: &storage_traits::EngineStats, name: &str) -> u64 {
    *stats.metrics.get(name).unwrap_or(&0)
}

/// After writing compressible data, compression counters must show that bytes
/// went through the compression path and the output is not larger than input.
#[test]
fn compression_counters_are_populated() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for i in 0..80usize {
        let key = format!("k{i:04}");
        let value = vec![b'x'; 100];
        engine.put(key.as_bytes(), &value).unwrap();
    }
    engine.sync().unwrap();

    let stats = engine.stats().unwrap();
    let bytes_in = get_metric(&stats, "compression_bytes_in");
    let bytes_out = get_metric(&stats, "compression_bytes_out");
    let blocks = get_metric(&stats, "compression_blocks");

    assert!(bytes_in > 0, "compression_bytes_in should be > 0");
    assert!(bytes_out > 0, "compression_bytes_out should be > 0");
    assert!(
        bytes_out <= bytes_in,
        "compressed bytes should not exceed input"
    );
    assert!(blocks > 0, "compression_blocks should be > 0");
}

/// Point reads must produce cache hit/miss and disk-read counters.
#[test]
fn cache_counters_track_reads() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for i in 0..80usize {
        let key = format!("k{i:04}");
        let value = vec![b'x'; 100];
        engine.put(key.as_bytes(), &value).unwrap();
    }
    engine.sync().unwrap();

    // First pass populates the cache; second pass should hit.
    for i in 0..80usize {
        let key = format!("k{i:04}");
        engine.get(key.as_bytes()).unwrap();
    }
    for i in 0..80usize {
        let key = format!("k{i:04}");
        engine.get(key.as_bytes()).unwrap();
    }

    let stats = engine.stats().unwrap();
    let hot_hits = get_metric(&stats, "cache_hot_hits");
    let hot_misses = get_metric(&stats, "cache_hot_misses");
    let disk_reads = get_metric(&stats, "cache_disk_reads");

    assert!(hot_hits > 0, "second pass should hit the hot tier");
    assert!(hot_misses > 0, "first pass should miss the hot tier");
    assert!(disk_reads > 0, "some blocks must be read from disk");
}

/// Compactions must update compaction counters.
#[test]
fn compaction_counters_are_populated() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for round in 0..5usize {
        for i in 0..80usize {
            let key = format!("k{i:04}");
            let value = format!("round-{round}-{i}");
            engine.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
    }
    engine.sync().unwrap();

    // Give background compactions time to run before sampling counters.
    std::thread::sleep(std::time::Duration::from_millis(300));

    let stats = engine.stats().unwrap();
    assert!(get_metric(&stats, "compaction_bytes_read") > 0);
    assert!(get_metric(&stats, "compaction_bytes_written") > 0);
    assert!(get_metric(&stats, "compaction_files_read") > 0);
    assert!(get_metric(&stats, "compaction_files_written") > 0);
}

/// `EngineStats` fields must be sensible.
#[test]
fn engine_stats_fields_are_sensible() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    // Write enough data to force at least one flush.
    for i in 0..80usize {
        let key = format!("k{i:04}");
        let value = vec![b'x'; 100];
        engine.put(key.as_bytes(), &value).unwrap();
    }
    engine.sync().unwrap();

    let stats = engine.stats().unwrap();
    assert_eq!(stats.name, "storage-kv");
    assert!(stats.disk_bytes > 0, "disk_bytes should include SSTables");
    assert!(stats.memory_bytes > 0, "memory_bytes should include caches");
}

/// Blob writes and GC must update blob-specific counters.
#[test]
fn blob_counters_are_populated() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        min_blob_value_size: 64,
        blob_file_size: 1,
        write_buffer_size: 64,
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 128,
        blob_gc_ratio: 1.0,
        blob_gc_interval_ms: 0,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    let key = b"k";
    let v1 = vec![b'1'; 100];
    let v2 = vec![b'2'; 100];

    engine.put(key, &v1).unwrap();
    engine.sync().unwrap();

    let stats_before = engine.stats().unwrap();
    assert!(get_metric(&stats_before, "blob_bytes_total") > 0);

    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();
    engine.run_blob_gc_once().unwrap();

    let stats_after = engine.stats().unwrap();
    assert!(get_metric(&stats_after, "blob_gc_scanned_files") > 0);
    assert!(get_metric(&stats_after, "blob_gc_deleted_files") > 0);
    assert!(get_metric(&stats_after, "blob_gc_space_reclaimed") > 0);
    assert_eq!(get_metric(&stats_after, "blob_bytes_garbage"), 0);
}

/// Blob references rewritten during compaction must update compaction-rewrite
/// counters and keep the value readable.
#[test]
fn blob_compaction_rewrite_counters_are_populated() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        min_blob_value_size: 64,
        blob_file_size: 1,
        write_buffer_size: 64,
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 128,
        blob_gc_ratio: 1.0,
        blob_gc_interval_ms: 0,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    let key = b"k";
    let v1 = vec![b'1'; 100];
    let v2 = vec![b'2'; 100];

    engine.put(key, &v1).unwrap();
    engine.sync().unwrap();

    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();

    // Trigger background compaction.
    for i in 0..20usize {
        let k = format!("x{i:03}");
        engine.put(k.as_bytes(), &[i as u8; 100]).unwrap();
    }
    engine.sync().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));

    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2)));

    let stats = engine.stats().unwrap();
    assert!(
        get_metric(&stats, "blob_compaction_rewritten_records") > 0
            || get_metric(&stats, "blob_gc_rewritten_records") > 0,
        "blob ref should have been rewritten by compaction or GC"
    );
}

/// A disabled cold tier must still report all metrics correctly.
#[test]
fn metrics_work_with_cold_tier_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = opts();
    opts.compressed_block_cache_size = 0;

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..40usize {
        let key = format!("k{i:04}");
        engine.put(key.as_bytes(), key.as_bytes()).unwrap();
    }
    engine.sync().unwrap();

    for i in 0..40usize {
        let key = format!("k{i:04}");
        assert_eq!(engine.get(key.as_bytes()).unwrap(), Some(Bytes::from(key)));
    }

    let stats = engine.stats().unwrap();
    assert!(get_metric(&stats, "cache_hot_hits") + get_metric(&stats, "cache_hot_misses") > 0);
    assert!(get_metric(&stats, "compression_blocks") > 0);
}
