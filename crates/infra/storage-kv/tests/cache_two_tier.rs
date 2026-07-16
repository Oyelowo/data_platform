//! Two-tier block cache behavior.
//!
//! The engine is configured with both a small hot tier (decompressed blocks)
//! and a larger cold tier (blocks exactly as stored on disk).  Flushes,
//! compactions, and reopening must keep returning the same data.  The cold
//! tier is normally disabled by default because the OS page cache already
//! holds the compressed file contents; enabling it here exercises the path
//! that decompresses from stored bytes.

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

/// Reads, scans, and reopens stay correct while both cache tiers are active
/// and compactions move data between files.
#[test]
fn two_tier_cache_keeps_data_correct_through_compactions() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    const KEYS: usize = 80;
    const ROUNDS: usize = 5;

    for round in 0..ROUNDS {
        for i in 0..KEYS {
            let key = format!("k{i:04}");
            let value = format!("round-{round}-value-{i}");
            engine.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
    }
    engine.sync().unwrap();

    // Shut down and reopen to quiesce all background compactions before
    // reading.  This keeps the test deterministic while still exercising
    // reads against files produced by compactions with both cache tiers.
    drop(engine);
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    // Point reads must return the latest value.
    for i in 0..KEYS {
        let key = format!("k{i:04}");
        let expected = format!("round-{last}-value-{i}", last = ROUNDS - 1);
        assert_eq!(
            engine.get(key.as_bytes()).unwrap(),
            Some(Bytes::from(expected)),
            "wrong value for {key}"
        );
    }

    // Scans must visit every distinct key exactly once.
    let mut cursor = engine.scan(None, None).unwrap();
    let mut seen = 0usize;
    while let Some(Ok((k, v))) = cursor.next() {
        assert_eq!(k.as_ref(), format!("k{seen:04}").as_bytes());
        assert_eq!(
            v.as_ref(),
            format!("round-{last}-value-{seen}", last = ROUNDS - 1).as_bytes()
        );
        seen += 1;
    }
    assert_eq!(seen, KEYS);

    // Reopen, then read again.  Recovery must work with the cache tiers
    // enabled and must not confuse old cached block keys with recovered files.
    drop(engine);
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..KEYS {
        let key = format!("k{i:04}");
        let expected = format!("round-{last}-value-{i}", last = ROUNDS - 1);
        assert_eq!(
            engine.get(key.as_bytes()).unwrap(),
            Some(Bytes::from(expected)),
            "wrong value for {key} after reopen"
        );
    }
}

/// The cold tier can be disabled entirely; the engine must still behave
/// correctly with only the hot tier.
#[test]
fn cold_tier_disabled_still_correct() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = opts();
    opts.compressed_block_cache_size = 0;

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..40usize {
        let key = format!("k{i:04}");
        let value = format!("v{i}");
        engine.put(key.as_bytes(), value.as_bytes()).unwrap();
    }
    engine.sync().unwrap();

    for i in 0..40usize {
        let key = format!("k{i:04}");
        assert_eq!(
            engine.get(key.as_bytes()).unwrap(),
            Some(Bytes::from(format!("v{i}"))),
            "wrong value for {key}"
        );
    }
}
