//! Criterion benchmarks for `storage-btree`.

use criterion::{Criterion, criterion_group, criterion_main};
use storage_btree::{BtreeEngine, BtreeOptions};
use storage_traits::{Engine, Transaction, TxnOptions};

fn bench_point_gets(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..10_000u32 {
        let key = format!("key{:08}", i);
        tx.put(key.as_bytes(), b"value").unwrap();
    }
    tx.commit().unwrap();

    c.bench_function("btree_point_get", |b| {
        b.iter(|| {
            let key = format!("key{:08}", criterion::black_box(5_000));
            let _ = engine.get(key.as_bytes());
        })
    });
}

fn bench_range_scan(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..10_000u32 {
        let key = format!("key{:08}", i);
        tx.put(key.as_bytes(), b"value").unwrap();
    }
    tx.commit().unwrap();

    c.bench_function("btree_range_scan_1000", |b| {
        b.iter(|| {
            let cursor = engine
                .scan(Some(b"key00000000"), Some(b"key00001000"))
                .unwrap();
            let count = cursor.take(1000).count();
            criterion::black_box(count);
        })
    });
}

criterion_group!(benches, bench_point_gets, bench_range_scan);
criterion_main!(benches);
