//! Durable full-text search engine.
//!
//! `storage-search` provides an embeddable, crash-safe search engine with:
//!
//! * Schema-defined fields with stored/indexed/tokenized/stemmed options.
//! * Inverted index with term positions for phrase queries.
//! * BM25 ranking with configurable `k1` and `b`.
//! * Boolean, phrase, prefix, and fielded queries.
//! * Segment-based persistence with WAL-backed durability.
//! * A `storage_traits::Engine` implementation for byte-key access.
//!
//! # Example
//!
//! ```rust,no_run
//! use storage_search::{SearchEngine, SearchOptions, Schema, FieldOptions, Document};
//!
//! let dir = tempfile::tempdir().unwrap();
//! let schema = Schema::new().with_field("title", FieldOptions::text());
//! let opts = SearchOptions::default_for(schema.clone());
//! let engine = SearchEngine::open(dir.path(), opts, schema).unwrap();
//! engine.index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world")).unwrap();
//! let results = engine.search("hello", None).unwrap();
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod analyzer;
pub mod compaction;
pub mod cursor;
pub mod document;
pub mod engine;
pub mod error;
pub mod format;
pub mod index;
pub mod options;
pub mod posting;
pub mod query;
pub mod recovery;
pub mod schema;
pub mod stats;
pub mod stemmer;
pub mod term;
pub mod tokenizer;
pub mod transaction;
pub mod wal;

pub use document::Document;
pub use engine::SearchEngine;
pub use error::{Error, Result};
pub use options::{SearchOptions, WalSyncPolicy};
pub use query::{Query, SearchResult};
pub use schema::{FieldOptions, FieldValue, Schema};
pub use stats::SearchStats;
pub use transaction::SearchTransaction;
