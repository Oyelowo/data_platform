//! Compression integration tests.
//!
//! Every supported codec must round-trip through the SSTable builder/reader
//! and through the full engine (flush, reopen, compaction).  Corrupt or
//! unknown-compression blocks must be rejected, and compaction outputs at the
//! bottommost level must use the configured bottommost codec.

use std::path::Path;
use std::time::{Duration, Instant};

use bytes::Bytes;
use storage_kv::internal_key::{ValueType, build_internal_key};
use storage_kv::sstable::builder::{SSTableBuilder, SSTableBuilderOptions};
use storage_kv::sstable::format::CompressionType;
use storage_kv::sstable::reader::SSTableReader;
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

const ALL_CODECS: [CompressionType; 4] = [
    CompressionType::None,
    CompressionType::Lz4,
    CompressionType::Zstd,
    CompressionType::Snappy,
];

/// Highly compressible value: 64-byte runs of one letter, cycling through the
/// alphabet.  Compresses well under every supported codec.
fn compressible_value(seed: usize, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| b'a' + ((seed + i / 64) % 26) as u8)
        .collect()
}

fn ikey(user_key: &[u8], seq: u64) -> Vec<u8> {
    build_internal_key(user_key, seq, ValueType::Value)
}

fn builder_opts(compression: CompressionType) -> SSTableBuilderOptions {
    SSTableBuilderOptions {
        block_size: 1024,
        compression,
        ..Default::default()
    }
}

/// Build an SSTable with `count` keys and return its metadata.
fn build_table(
    dir: &Path,
    number: u64,
    compression: CompressionType,
    count: u32,
) -> storage_kv::sstable::builder::BuiltSSTable {
    let path = dir.join(format!("{number:06}.sst"));
    let mut builder = SSTableBuilder::open(&path, builder_opts(compression)).unwrap();
    for i in 0..count {
        let key = ikey(format!("key-{i:05}").as_bytes(), 1);
        let value = compressible_value(i as usize, 512);
        builder.add(&key, &value).unwrap();
    }
    builder.finish().unwrap()
}

#[test]
fn builder_reader_roundtrip_every_codec() {
    for ty in ALL_CODECS {
        let dir = tempfile::tempdir().unwrap();
        let built = build_table(dir.path(), 1, ty, 200);
        assert_eq!(built.num_entries, 200);

        let mut reader = SSTableReader::open(&built.path, 1, None).unwrap();
        for i in 0..200u32 {
            let user_key = format!("key-{i:05}");
            let got = reader.get(user_key.as_bytes(), u64::MAX).unwrap();
            assert_eq!(
                got,
                Some(Some(Bytes::from(compressible_value(i as usize, 512)))),
                "codec {ty:?} lost key {user_key}"
            );
        }

        // Full iteration must yield every entry in order.
        let mut iter = reader.iter().unwrap();
        iter.seek_to_first().unwrap();
        let mut seen = 0u32;
        while iter.valid() {
            let expected = ikey(format!("key-{seen:05}").as_bytes(), 1);
            assert_eq!(iter.key(), expected.as_slice(), "codec {ty:?} iter order");
            iter.next().unwrap();
            seen += 1;
        }
        assert_eq!(seen, 200, "codec {ty:?} iter count");
    }
}

#[test]
fn compressible_data_actually_shrinks() {
    let dir = tempfile::tempdir().unwrap();
    let plain = build_table(dir.path(), 1, CompressionType::None, 200);
    for ty in [
        CompressionType::Lz4,
        CompressionType::Zstd,
        CompressionType::Snappy,
    ] {
        let compressed = build_table(dir.path(), 2, ty, 200);
        assert!(
            compressed.file_size * 2 < plain.file_size,
            "codec {ty:?} did not shrink the table: {} vs {}",
            compressed.file_size,
            plain.file_size
        );
    }
}

#[test]
fn engine_roundtrip_every_codec() {
    for ty in ALL_CODECS {
        let dir = tempfile::tempdir().unwrap();
        let opts = LsmOptions {
            write_buffer_size: 4 * 1024, // tiny: forces flushes
            level0_file_num_compaction_trigger: 2,
            compression: ty,
            bottommost_compression: ty,
            ..Default::default()
        };
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        for i in 0..100u32 {
            let value = compressible_value(i as usize, 1024);
            engine.put(format!("k{i:04}").as_bytes(), &value).unwrap();
        }
        // Point reads while data is spread across memtable + L0 files.
        for i in 0..100u32 {
            let got = engine.get(format!("k{i:04}").as_bytes()).unwrap();
            assert_eq!(
                got,
                Some(Bytes::from(compressible_value(i as usize, 1024))),
                "codec {ty:?} lost k{i:04} before reopen"
            );
        }
        engine.sync().unwrap();
        drop(engine);

        // Reopen: everything comes back from SSTables on disk.
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        for i in 0..100u32 {
            let got = engine.get(format!("k{i:04}").as_bytes()).unwrap();
            assert_eq!(
                got,
                Some(Bytes::from(compressible_value(i as usize, 1024))),
                "codec {ty:?} lost k{i:04} after reopen"
            );
        }
        let mut cursor = engine.scan(None, None).unwrap();
        let mut seen = 0u32;
        while let Some(Ok((_k, _v))) = cursor.next() {
            seen += 1;
        }
        assert_eq!(seen, 100, "codec {ty:?} scan count after reopen");
    }
}

#[test]
fn corrupt_compressed_block_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    // One key with a large compressible value, so the first data block has a
    // sizeable compressed payload at the start of the file.
    let path = dir.path().join("000001.sst");
    let mut builder = SSTableBuilder::open(&path, builder_opts(CompressionType::Lz4)).unwrap();
    builder
        .add(&ikey(b"victim", 1), &compressible_value(0, 8 * 1024))
        .unwrap();
    builder.finish().unwrap();

    // Corrupt a byte inside the first data block's compressed payload
    // (offset 16: past the 4-byte length prefix, inside the LZ4 stream).
    let mut bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 32);
    bytes[16] ^= 0xFF;
    std::fs::write(&path, bytes).unwrap();

    let mut reader = SSTableReader::open(&path, 1, None).unwrap();
    let err = reader.get(b"victim", u64::MAX).unwrap_err();
    assert!(
        err.to_string().contains("checksum"),
        "expected checksum error, got: {err}"
    );
}

#[test]
fn unknown_compression_type_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("000001.sst");
    let mut builder = SSTableBuilder::open(&path, builder_opts(CompressionType::None)).unwrap();
    builder.add(&ikey(b"a", 1), b"1").unwrap();
    builder.finish().unwrap();

    // Parse the index handle out of the 48-byte footer:
    // [metaindex handle (16)][index handle (16)][version (8)][magic (8)].
    let mut bytes = std::fs::read(&path).unwrap();
    let footer_start = bytes.len() - 48;
    let index_offset = u64::from_le_bytes(
        bytes[footer_start + 16..footer_start + 24]
            .try_into()
            .unwrap(),
    ) as usize;
    let index_size = u64::from_le_bytes(
        bytes[footer_start + 24..footer_start + 32]
            .try_into()
            .unwrap(),
    ) as usize;
    // The first byte of the index block's trailer is the compression type.
    bytes[index_offset + index_size] = 99;
    std::fs::write(&path, bytes).unwrap();

    let err = match SSTableReader::open(&path, 1, None) {
        Ok(_) => panic!("reader must reject an unknown compression type"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("unknown compression"),
        "expected unknown-compression error, got: {err}"
    );
}

/// Scan the data directory for an SSTable containing a zstd frame magic.
fn dir_contains_zstd_frame(dir: &Path) -> bool {
    const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD]; // LE 0xFD2FB528
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("sst") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        if bytes.windows(4).any(|w| w == ZSTD_MAGIC) {
            return true;
        }
    }
    false
}

#[test]
fn bottommost_compaction_uses_bottommost_codec() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 4 * 1024,
        level0_file_num_compaction_trigger: 2,
        num_levels: 2, // L1 is the bottommost level
        compression: CompressionType::Lz4,
        bottommost_compression: CompressionType::Zstd,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for round in 0..3u32 {
        for i in 0..40u32 {
            let n = round * 40 + i;
            engine
                .put(
                    format!("k{n:04}").as_bytes(),
                    &compressible_value(n as usize, 1024),
                )
                .unwrap();
        }
        engine.sync().unwrap();
    }

    // Compaction runs in the background; wait for an L1 output carrying zstd
    // frames to appear.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !dir_contains_zstd_frame(dir.path()) {
        assert!(
            Instant::now() < deadline,
            "no zstd-compressed SSTable appeared within 10s"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Every key must still read correctly through the zstd decode path.
    for n in 0..120u32 {
        let got = engine.get(format!("k{n:04}").as_bytes()).unwrap();
        assert_eq!(
            got,
            Some(Bytes::from(compressible_value(n as usize, 1024))),
            "lost k{n:04} after bottommost compaction"
        );
    }
    engine.sync().unwrap();
    drop(engine);

    // Cold re-read after reopen (fresh block cache).
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for n in 0..120u32 {
        let got = engine.get(format!("k{n:04}").as_bytes()).unwrap();
        assert_eq!(
            got,
            Some(Bytes::from(compressible_value(n as usize, 1024))),
            "lost k{n:04} after reopen"
        );
    }
}

/// Blocks written through the engine must be readable with an engine-level
/// block cache enabled, exercising the cache-insert path after decompression.
#[test]
fn compressed_blocks_flow_through_block_cache() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 4 * 1024,
        block_cache_size: 1024 * 1024,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..50u32 {
        engine
            .put(
                format!("k{i:03}").as_bytes(),
                &compressible_value(i as usize, 2048),
            )
            .unwrap();
    }
    engine.sync().unwrap();
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    // Two passes: the first populates the cache from disk, the second must
    // serve identical data from cached decompressed blocks.
    for _ in 0..2 {
        for i in 0..50u32 {
            let got = engine.get(format!("k{i:03}").as_bytes()).unwrap();
            assert_eq!(
                got,
                Some(Bytes::from(compressible_value(i as usize, 2048))),
                "wrong value for k{i:03}"
            );
        }
    }
}
