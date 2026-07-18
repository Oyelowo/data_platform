//! Criterion benchmarks for the v2 in-place B+ tree engine.

use criterion::{Criterion, criterion_group, criterion_main};
use storage_btree::{BtreeEngine, BtreeOptions};
use storage_traits::{Engine, Transaction, TxnOptions};

const DATASET_SIZE: u32 = 10_000;

fn bench_point_gets(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..DATASET_SIZE {
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

fn bench_point_puts(c: &mut Criterion) {
    c.bench_function("btree_point_put", |b| {
        b.iter_with_setup(
            || {
                let dir = tempfile::tempdir().unwrap();
                let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
                (dir, engine)
            },
            |(_dir, engine)| {
                let mut tx = engine.begin(TxnOptions::default()).unwrap();
                let key = format!("key{:08}", criterion::black_box(42));
                tx.put(key.as_bytes(), b"value").unwrap();
                tx.commit().unwrap();
            },
        )
    });
}

fn bench_range_scan(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..DATASET_SIZE {
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

fn bench_write_throughput(c: &mut Criterion) {
    c.bench_function("btree_write_throughput_1000", |b| {
        b.iter_with_setup(
            || {
                let dir = tempfile::tempdir().unwrap();
                let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
                (dir, engine)
            },
            |(_dir, engine)| {
                let mut tx = engine.begin(TxnOptions::default()).unwrap();
                for i in 0..1_000u32 {
                    let key = format!("key{:08}", i);
                    tx.put(key.as_bytes(), b"value").unwrap();
                }
                tx.commit().unwrap();
            },
        )
    });
}

fn bench_mixed_workload(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..DATASET_SIZE {
        let key = format!("key{:08}", i);
        tx.put(key.as_bytes(), b"value").unwrap();
    }
    tx.commit().unwrap();

    let mut counter: u32 = 0;
    c.bench_function("btree_mixed_80_20", |b| {
        b.iter(|| {
            counter = counter.wrapping_add(1);
            if counter.is_multiple_of(5) {
                let mut tx = engine.begin(TxnOptions::default()).unwrap();
                let key = format!("key{:08}", counter % DATASET_SIZE);
                tx.put(key.as_bytes(), b"updated").unwrap();
                tx.commit().unwrap();
            } else {
                let key = format!("key{:08}", counter % DATASET_SIZE);
                let _ = engine.get(key.as_bytes());
            }
        })
    });
}

criterion_group!(
    benches,
    bench_point_gets,
    bench_point_puts,
    bench_range_scan,
    bench_write_throughput,
    bench_mixed_workload
);
criterion_main!(benches);
