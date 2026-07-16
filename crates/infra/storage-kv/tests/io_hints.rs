//! I/O hint tests: fadvise hints (random for point reads, sequential for
//! scans/iteration) must never change read results or destabilize the engine.

use bytes::Bytes;
use storage_kv::internal_key::{ValueType, build_internal_key};
use storage_kv::sstable::builder::{SSTableBuilder, SSTableBuilderOptions};
use storage_kv::sstable::reader::SSTableReader;
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

fn ikey(user_key: &[u8], seq: u64) -> Vec<u8> {
    build_internal_key(user_key, seq, ValueType::Value)
}

/// Opening a reader applies the `Random` hint and `iter()` the `Sequential`
/// hint; reads must return identical data regardless.
#[test]
fn hints_do_not_change_results() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("000001.sst");
    let mut builder = SSTableBuilder::open(&path, SSTableBuilderOptions::default()).unwrap();
    let mut expected = Vec::new();
    for i in 0..300u32 {
        let key = ikey(format!("h{i:05}").as_bytes(), 1);
        let value = format!("value-{i}").into_bytes();
        builder.add(&key, &value).unwrap();
        expected.push((format!("h{i:05}"), value));
    }
    builder.finish().unwrap();

    // Point reads (Random hint applied at open).
    let mut reader = SSTableReader::open(&path, 1, None).unwrap();
    for (user_key, value) in &expected {
        assert_eq!(
            reader.get(user_key.as_bytes(), u64::MAX).unwrap(),
            Some(Some(Bytes::copy_from_slice(value))),
            "point read wrong for {user_key}"
        );
    }

    // Full iteration (Sequential hint applied by iter()).
    let mut iter = reader.iter().unwrap();
    iter.seek_to_first().unwrap();
    let mut seen = 0usize;
    while iter.valid() {
        let (user_key, value) = &expected[seen];
        assert_eq!(iter.key(), ikey(user_key.as_bytes(), 1).as_slice());
        assert_eq!(iter.value(), value.as_slice());
        iter.next().unwrap();
        seen += 1;
    }
    assert_eq!(seen, expected.len());

    // Point reads must still be correct after the iterator re-hinted the file.
    for (user_key, value) in expected.iter().step_by(7) {
        assert_eq!(
            reader.get(user_key.as_bytes(), u64::MAX).unwrap(),
            Some(Some(Bytes::copy_from_slice(value))),
            "point read after iteration wrong for {user_key}"
        );
    }
}

/// Engine-level: writes, flushes, reopens and scans with hints active
/// throughout must all observe the same data.
#[test]
fn engine_reads_stable_with_hints() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 4 * 1024,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..80u32 {
        engine
            .put(format!("k{i:04}").as_bytes(), format!("v{i}").as_bytes())
            .unwrap();
    }
    engine.sync().unwrap();
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..80u32 {
        assert_eq!(
            engine.get(format!("k{i:04}").as_bytes()).unwrap(),
            Some(Bytes::from(format!("v{i}"))),
            "lost k{i:04}"
        );
    }
    let mut cursor = engine.scan(None, None).unwrap();
    let mut seen = 0u32;
    while let Some(Ok((k, v))) = cursor.next() {
        let i = seen;
        assert_eq!(k.as_ref(), format!("k{i:04}").as_bytes());
        assert_eq!(v.as_ref(), format!("v{i}").as_bytes());
        seen += 1;
    }
    assert_eq!(seen, 80);
}
