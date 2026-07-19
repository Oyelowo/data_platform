//! `SearchEngine` and its `storage_traits::Engine` implementation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use storage_traits::{
    BoundKind, Engine, EngineStats, Error as TraitError, Result as TraitResult, TxnOptions,
};

use crate::cursor::SearchCursor;
use crate::document::{DocId, Document, decode_document, encode_document};
use crate::error::Error;
use crate::format::{
    Metadata, WalRecord, META_FILE, SEGMENTS_DIR, decode_engine_key, decode_field_value,
    encode_engine_key, encode_field_value,
};
use crate::index::memory::MemorySegment;
use crate::index::merger::merge_segments;
use crate::index::segment::ImmutableSegment;
use crate::index::writer::SegmentWriter;
use crate::options::SearchOptions;
use crate::query::{
    Query, SearchResult, execute,
    scorer::{Bm25Scorer},
};
use crate::recovery;
use crate::schema::Schema;
use crate::stats::SearchStats;
use crate::transaction::SearchTransaction;
use crate::wal::SearchWal;

/// Inner engine state shared between the public handle and transactions.
pub(crate) struct Inner {
    pub dir: PathBuf,
    pub options: SearchOptions,
    pub schema: Schema,
    pub metadata: RwLock<Metadata>,
    pub memory: Mutex<MemorySegment>,
    pub segments: RwLock<Vec<ImmutableSegment>>,
    pub wal: SearchWal,
    pub write_lock: Mutex<()>,
    pub next_segment_id: Mutex<u64>,
}

/// A synchronous, durable full-text search engine.
#[derive(Clone)]
pub struct SearchEngine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for SearchEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchEngine")
            .field("dir", &self.inner.dir)
            .field("options", &self.inner.options)
            .field("schema", &self.inner.schema)
            .finish()
    }
}

impl SearchEngine {
    /// Open or create a search engine at `dir` with `options` and `schema`.
    pub fn open(
        dir: impl AsRef<Path>,
        options: SearchOptions,
        schema: Schema,
    ) -> crate::Result<Self> {
        options.validate()?;
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        std::fs::create_dir_all(dir.join(SEGMENTS_DIR))?;

        let meta_path = dir.join(META_FILE);
        let mut metadata = if meta_path.exists() {
            let bytes = std::fs::read(&meta_path)?;
            Metadata::decode(&bytes)?
        } else {
            Metadata::new(options.clone(), schema.clone())
        };

        // Validate on-disk options and schema match requested.
        if metadata.schema != schema {
            return Err(Error::invalid_argument(
                "cannot open engine with different schema",
            ));
        }
        metadata.options = options.clone();
        metadata.schema = schema.clone();

        let wal = SearchWal::open(&dir, options.wal_sync_policy)?;
        let mut memory = MemorySegment::new();
        recovery::replay(&wal, &mut memory, &mut metadata)?;

        let segments = load_segments(&dir, &metadata.segment_ids)?;
        let next_segment_id = metadata
            .segment_ids
            .last()
            .copied()
            .unwrap_or(0)
            .saturating_add(1);

        let engine = Self {
            inner: Arc::new(Inner {
                dir,
                options,
                schema,
                metadata: RwLock::new(metadata.clone()),
                memory: Mutex::new(memory),
                segments: RwLock::new(segments),
                wal,
                write_lock: Mutex::new(()),
                next_segment_id: Mutex::new(next_segment_id),
            }),
        };
        engine.persist_meta()?;
        Ok(engine)
    }

    pub(crate) fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }

    /// Index or replace a document.
    pub fn index_document(
        &self,
        doc_id: impl Into<Vec<u8>>,
        document: Document,
    ) -> crate::Result<()> {
        let doc_id = doc_id.into();
        if doc_id.len() > self.inner.options.max_key_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Key,
                limit: self.inner.options.max_key_len,
                got: doc_id.len(),
            }
            .into());
        }
        let _guard = self.inner.write_lock.lock();
        self.index_document_unlocked(&doc_id, document)
    }

    pub(crate) fn index_document_unlocked(
        &self,
        doc_id: &DocId,
        document: Document,
    ) -> crate::Result<()> {
        document.validate(&self.inner.schema)?;
        let record = WalRecord::IndexDocument {
            doc_id: doc_id.clone(),
            document: document.clone(),
        };
        self.inner.wal.append(record)?;
        self.inner
            .memory
            .lock()
            .index_document(doc_id.clone(), &document, &self.inner.schema)?;
        self.maybe_flush_memory()?;
        Ok(())
    }

    /// Delete a document.
    pub fn delete_document(&self, doc_id: impl AsRef<[u8]>) -> crate::Result<bool> {
        let _guard = self.inner.write_lock.lock();
        self.delete_unlocked(doc_id.as_ref())
    }

    pub(crate) fn put_unlocked(&self, key: &[u8], value: &[u8]) -> crate::Result<()> {
        let (doc_id, field_name) = decode_engine_key(key)?;
        let field_value = decode_field_value(value)?;
        let mut doc = self.get_document(doc_id)?.unwrap_or_default();
        doc.fields.insert(field_name.to_string(), field_value);
        self.index_document_unlocked(&doc_id.to_vec(), doc)
    }

    pub(crate) fn delete_unlocked(&self, key: &[u8]) -> crate::Result<bool> {
        // Key may encode (doc_id, field) or be just doc_id.
        let doc_id = match decode_engine_key(key) {
            Ok((doc_id, _)) => doc_id.to_vec(),
            Err(_) => key.to_vec(),
        };

        self.inner
            .wal
            .append(WalRecord::DeleteDocument { doc_id: doc_id.clone() })?;
        self.inner.memory.lock().delete_document(&doc_id);

        // Also mark deleted in disk segments.
        for seg in self.inner.segments.write().iter_mut() {
            seg.delete_document(&doc_id);
        }
        Ok(true)
    }

    /// Get the stored document for a doc id.
    pub fn get_document(&self, doc_id: impl AsRef<[u8]>) -> crate::Result<Option<Document>> {
        let doc_id = doc_id.as_ref().to_vec();
        if let Some(doc) = self.inner.memory.lock().get_document(&doc_id) {
            return Ok(Some(doc));
        }
        for seg in self.inner.segments.read().iter() {
            if let Some(doc) = seg.get_document(&doc_id) {
                return Ok(Some(doc));
            }
        }
        Ok(None)
    }

    /// Search for documents matching `query` and return the top `k`.
    pub fn search(&self, query: &str, top_k: Option<usize>) -> crate::Result<Vec<SearchResult>> {
        let parsed = crate::query::parser::parse(query)?;
        let k = top_k.unwrap_or(self.inner.options.default_top_k);
        let scorer = Bm25Scorer::new(self.inner.options.bm25_k1, self.inner.options.bm25_b);
        let memory = self.inner.memory.lock();
        let segments = self.inner.segments.read();
        execute(&parsed, &self.inner.schema, &memory, &segments, &scorer, k)
    }

    /// Typed search using a [`Query`] AST.
    pub fn search_query(
        &self,
        query: &Query,
        top_k: Option<usize>,
    ) -> crate::Result<Vec<SearchResult>> {
        let k = top_k.unwrap_or(self.inner.options.default_top_k);
        let scorer = Bm25Scorer::new(self.inner.options.bm25_k1, self.inner.options.bm25_b);
        let memory = self.inner.memory.lock();
        let segments = self.inner.segments.read();
        execute(query, &self.inner.schema, &memory, &segments, &scorer, k)
    }

    /// Flush memory segment, run merge policy, persist metadata, and checkpoint WAL.
    pub fn sync(&self) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        self.flush_memory()?;
        self.run_merge_policy()?;
        self.persist_meta()?;
        self.inner.wal.sync()?;
        self.inner.wal.truncate_completed()?;
        Ok(())
    }

    fn maybe_flush_memory(&self) -> crate::Result<()> {
        if self.inner.memory.lock().approx_bytes() >= self.inner.options.memtable_size_limit {
            self.flush_memory()?;
        }
        Ok(())
    }

    fn flush_memory(&self) -> crate::Result<()> {
        let mut memory = self.inner.memory.lock();
        if memory.is_empty() {
            return Ok(());
        }
        let mut next_id = self.inner.next_segment_id.lock();
        let segment_id = *next_id;
        *next_id += 1;
        let segment_dir = self.inner.dir.join(SEGMENTS_DIR);
        let path = SegmentWriter::write(&segment_dir, segment_id, &memory)?;
        let segment = ImmutableSegment::open(&path)?;
        self.inner.segments.write().push(segment);
        {
            let mut meta = self.inner.metadata.write();
            meta.segment_ids.push(segment_id);
        }
        *memory = MemorySegment::new();
        Ok(())
    }

    fn run_merge_policy(&self) -> crate::Result<()> {
        let segments = self.inner.segments.read();
        if segments.len() <= self.inner.options.max_segments {
            return Ok(());
        }

        let sizes: Vec<(u64, usize)> = {
            let meta = self.inner.metadata.read();
            meta.segment_ids
                .iter()
                .zip(segments.iter())
                .map(|(&id, seg)| (id, seg.len()))
                .collect()
        };
        drop(segments);

        let to_merge = crate::compaction::select_segments_to_merge(
            &sizes,
            self.inner.options.max_segments,
            self.inner.options.merge_factor,
        );

        if let Some(ids) = to_merge {
            let mut segments = self.inner.segments.write();
            let mut meta = self.inner.metadata.write();
            let to_merge_segments: Vec<ImmutableSegment> = segments
                .drain(..)
                .filter(|seg| {
                    let id = segment_id_from_path(&seg.path);
                    id.is_some_and(|id| ids.contains(&id))
                })
                .collect();
            let remaining_ids: Vec<u64> = meta
                .segment_ids
                .drain(..)
                .filter(|id| !ids.contains(id))
                .collect();

            if !to_merge_segments.is_empty() {
                let mut next_id = self.inner.next_segment_id.lock();
                let new_id = *next_id;
                *next_id += 1;
                let segment_dir = self.inner.dir.join(SEGMENTS_DIR);
                let path = merge_segments(&segment_dir, new_id, &to_merge_segments)?;
                let segment = ImmutableSegment::open(&path)?;
                segments.push(segment);
                meta.segment_ids = remaining_ids;
                meta.segment_ids.push(new_id);

                // Remove old segment directories.
                for id in &ids {
                    let _ = std::fs::remove_dir_all(segment_dir.join(format!("segment_{id:016x}")));
                }
            }
        }

        Ok(())
    }

    /// Persist the current metadata file atomically.
    pub fn persist_meta(&self) -> crate::Result<()> {
        let meta = self.inner.metadata.read();
        let encoded = meta.encode()?;
        storage_file::atomic_write(&self.inner.dir.join(META_FILE), &encoded)?;
        Ok(())
    }

    /// Close the engine gracefully.
    pub fn close(&self) -> crate::Result<()> {
        self.sync()?;
        self.inner.wal.close()?;
        Ok(())
    }

    /// Return engine statistics.
    pub fn stats(&self) -> crate::Result<SearchStats> {
        let memory = self.inner.memory.lock();
        let segments = self.inner.segments.read();
        let num_docs = memory.doc_count() as u64 + segments.iter().map(|s| s.live_docs() as u64).sum::<u64>();
        let num_segments = segments.len() as u64;
        let memory_bytes = memory.approx_bytes() as u64;
        Ok(SearchStats {
            name: "storage-search",
            num_docs,
            num_segments,
            disk_bytes: approx_dir_bytes(&self.inner.dir)?,
            memory_bytes,
            metrics: {
                let mut m = std::collections::HashMap::new();
                m.insert("max_key_len".into(), self.inner.options.max_key_len as u64);
                m.insert("memory_docs".into(), memory.doc_count() as u64);
                m
            },
        })
    }
}

fn load_segments(dir: &Path, ids: &[u64]) -> crate::Result<Vec<ImmutableSegment>> {
    let mut segments = Vec::with_capacity(ids.len());
    for id in ids {
        let path = dir.join(SEGMENTS_DIR).join(format!("segment_{id:016x}"));
        if path.exists() {
            segments.push(ImmutableSegment::open(&path)?);
        }
    }
    Ok(segments)
}

fn segment_id_from_path(path: &Path) -> Option<u64> {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| {
            n.strip_prefix("segment_")
                .and_then(|s| u64::from_str_radix(s, 16).ok())
        })
}

fn approx_dir_bytes(dir: &Path) -> crate::Result<u64> {
    let mut total = 0u64;
    if let Ok(entries) = walkdir(dir) {
        for entry in entries {
            if let Ok(md) = entry.metadata() {
                total += md.len();
            }
        }
    }
    Ok(total)
}

fn walkdir(path: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
    fn collect(path: &Path, out: &mut Vec<std::fs::DirEntry>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(&entry.path(), out)?;
            } else {
                out.push(entry);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    collect(path, &mut out)?;
    Ok(out)
}

impl Engine for SearchEngine {
    type Error = Error;
    type Transaction = SearchTransaction;
    type Cursor = SearchCursor;

    fn name(&self) -> &'static str {
        "storage-search"
    }

    fn begin(&self, opts: TxnOptions) -> TraitResult<Self::Transaction, Self::Error> {
        Ok(SearchTransaction::new(self.clone(), opts))
    }

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        // Try (doc_id, field) encoding first.
        if let Ok((doc_id, field_name)) = decode_engine_key(key) {
            if let Some(doc) = self.get_document(doc_id)?
                && let Some(value) = doc.fields.get(field_name)
            {
                return Ok(Some(Bytes::from(encode_field_value(value))));
            }
            return Ok(None);
        }

        // Treat key as raw doc_id and return full stored document.
        match self.get_document(key)? {
            Some(doc) => Ok(Some(Bytes::from(encode_document(&doc)?))),
            None => Ok(None),
        }
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<Self::Cursor, Self::Error> {
        let mut map: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

        // Collect from memory segment.
        let memory = self.inner.memory.lock();
        for (doc_id, bytes) in memory.stored_docs() {
            if let Ok(doc) = decode_document(bytes) {
                for (field_name, value) in &doc.fields {
                    let key = encode_engine_key(doc_id, field_name);
                    if key_in_range(&key, start, end) {
                        map.insert(key, encode_field_value(value));
                    }
                }
            }
        }
        drop(memory);

        // Collect from disk segments.
        for seg in self.inner.segments.read().iter() {
            for doc_id in seg.live_doc_ids() {
                if let Some(doc) = seg.get_document(&doc_id) {
                    for (field_name, value) in &doc.fields {
                        let key = encode_engine_key(&doc_id, field_name);
                        if key_in_range(&key, start, end) {
                            map.insert(key, encode_field_value(value));
                        }
                    }
                }
            }
        }

        Ok(SearchCursor::from_map(map))
    }

    fn stats(&self) -> TraitResult<EngineStats, Self::Error> {
        let s = self.stats()?;
        Ok(s.into_engine_stats())
    }

    fn sync(&self) -> TraitResult<(), Self::Error> {
        SearchEngine::sync(self)
    }
}

fn key_in_range(key: &[u8], start: Option<&[u8]>, end: Option<&[u8]>) -> bool {
    let above_start = start.map(|s| key >= s).unwrap_or(true);
    let below_end = end.map(|e| key < e).unwrap_or(true);
    above_start && below_end
}

impl From<TraitError> for Error {
    fn from(e: TraitError) -> Self {
        match e {
            TraitError::Io(io) => Error::Io(io),
            TraitError::OutOfBounds { kind, limit, got } => Error::OutOfBounds {
                kind: match kind {
                    BoundKind::Key => "key",
                    BoundKind::Value => "value",
                    BoundKind::Batch => "batch",
                },
                limit,
                got,
            },
            TraitError::InactiveTransaction => Error::InactiveTransaction,
            TraitError::ReadOnlyTransaction => Error::ReadOnlyTransaction,
            TraitError::Unsupported(msg) => Error::Unsupported(msg),
            TraitError::Corruption(msg) => Error::Corruption(msg),
            TraitError::NotFound(msg) => Error::NotFound(msg),
            TraitError::Conflict(msg) => Error::Conflict(msg),
            _ => Error::InvalidArgument("unknown trait error".into()),
        }
    }
}
