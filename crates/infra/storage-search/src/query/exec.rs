//! Query execution across memory and disk segments.

use std::collections::{HashMap, HashSet};

use crate::document::{DocId, Document};
use crate::index::memory::MemorySegment;
use crate::index::segment::ImmutableSegment;
use crate::posting::Posting;
use crate::query::Query;
use crate::query::scorer::{Bm25Scorer, SegmentSource, positions_match_phrase};
use crate::schema::{FieldOptions, Schema};
use crate::stemmer::stem;
use crate::term::Term;

/// A single search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// Document identifier.
    pub doc_id: Vec<u8>,
    /// BM25 score.
    pub score: f32,
    /// Stored document, if available.
    pub document: Option<Document>,
}

/// Execute a query against a memory segment and a list of disk segments.
pub fn execute(
    query: &Query,
    schema: &Schema,
    memory: &MemorySegment,
    segments: &[ImmutableSegment],
    scorer: &Bm25Scorer,
    top_k: usize,
) -> crate::Result<Vec<SearchResult>> {
    let mut all_scores: HashMap<DocId, f32> = HashMap::new();
    let mut all_docs: HashMap<DocId, Option<Document>> = HashMap::new();

    for source in segment_sources(memory, segments) {
        let matches = search_segment(query, schema, source.as_ref(), scorer)?;
        for (doc_id, score) in matches {
            let doc = source.stored(&doc_id);
            *all_scores.entry(doc_id.clone()).or_default() += score;
            all_docs.entry(doc_id).or_insert(doc);
        }
    }

    let mut results: Vec<SearchResult> = all_scores
        .into_iter()
        .map(|(doc_id, score)| SearchResult {
            doc_id: doc_id.clone(),
            score,
            document: all_docs.remove(&doc_id).flatten(),
        })
        .collect();

    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results.truncate(top_k);
    Ok(results)
}

fn segment_sources<'a>(
    memory: &'a MemorySegment,
    segments: &'a [ImmutableSegment],
) -> Vec<Box<dyn SegmentSource<Error = crate::Error> + 'a>> {
    let mut sources: Vec<Box<dyn SegmentSource<Error = crate::Error> + 'a>> = Vec::new();
    sources.push(Box::new(MemorySource(memory)));
    for seg in segments {
        sources.push(Box::new(ImmutableSource(seg)));
    }
    sources
}

struct MemorySource<'a>(&'a MemorySegment);
struct ImmutableSource<'a>(&'a ImmutableSegment);

impl<'a> SegmentSource for MemorySource<'a> {
    type Error = crate::Error;

    fn postings(&self, term: &Term) -> Result<Vec<Posting>, Self::Error> {
        Ok(self.0.postings(term).to_vec())
    }

    fn prefix_postings(
        &self,
        field: Option<&str>,
        prefix: &str,
    ) -> Result<Vec<(Term, Vec<Posting>)>, Self::Error> {
        let mut out = Vec::new();
        for (term, postings) in self.0.terms() {
            let field_match = field.is_none_or(|f| term.field == f);
            if field_match && term.token.starts_with(prefix) {
                out.push((term.clone(), postings.clone()));
            }
        }
        Ok(out)
    }

    fn avg_field_length(&self, field: &str) -> f32 {
        let mut sum = 0u64;
        let mut count = 0u32;
        for ((_, f), norm) in self.0.norms() {
            if f == field {
                sum += u64::from(*norm);
                count += 1;
            }
        }
        if count == 0 {
            return 1.0;
        }
        (sum as f32 / count as f32).max(1.0)
    }

    fn total_docs(&self) -> usize {
        self.0.doc_count()
    }

    fn doc_freq(&self, term: &Term) -> usize {
        self.0.postings(term).len()
    }

    fn norm(&self, doc_id: &DocId, field: &str) -> u32 {
        self.0.norm(doc_id, field)
    }

    fn stored(&self, doc_id: &DocId) -> Option<Document> {
        self.0.get_document(doc_id)
    }

    fn all_doc_ids(&self) -> Vec<DocId> {
        self.0
            .stored_docs()
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl<'a> SegmentSource for ImmutableSource<'a> {
    type Error = crate::Error;

    fn postings(&self, term: &Term) -> Result<Vec<Posting>, Self::Error> {
        self.0.postings(term)
    }

    fn prefix_postings(
        &self,
        field: Option<&str>,
        prefix: &str,
    ) -> Result<Vec<(Term, Vec<Posting>)>, Self::Error> {
        self.0.prefix_postings(field, prefix)
    }

    fn avg_field_length(&self, field: &str) -> f32 {
        self.0.avg_field_length(field)
    }

    fn total_docs(&self) -> usize {
        self.0.len()
    }

    fn doc_freq(&self, term: &Term) -> usize {
        self.0.doc_freq(term)
    }

    fn norm(&self, doc_id: &DocId, field: &str) -> u32 {
        self.0.norm(doc_id, field)
    }

    fn stored(&self, doc_id: &DocId) -> Option<Document> {
        self.0.get_document(doc_id)
    }

    fn all_doc_ids(&self) -> Vec<DocId> {
        self.0.live_doc_ids()
    }
}

fn search_segment(
    query: &Query,
    schema: &Schema,
    source: &dyn SegmentSource<Error = crate::Error>,
    scorer: &Bm25Scorer,
) -> crate::Result<HashMap<DocId, f32>> {
    match query {
        Query::Term { field, term } => {
            let terms = expand_term_query(field.as_deref(), term, schema)?;
            score_terms(source, scorer, &terms)
        }
        Query::Phrase { field, terms } => {
            let field_terms = expand_phrase_query(field.as_deref(), terms, schema)?;
            score_phrase(source, scorer, &field_terms)
        }
        Query::Prefix { field, prefix } => {
            score_prefix(source, scorer, field.as_deref(), prefix)
        }
        Query::Boolean {
            must,
            should,
            must_not,
        } => score_boolean(source, scorer, schema, must, should, must_not),
    }
}

fn expand_term_query(
    field: Option<&str>,
    term: &str,
    schema: &Schema,
) -> crate::Result<Vec<Term>> {
    if let Some(field_name) = field {
        let options = schema.get(field_name).ok_or_else(|| {
            crate::Error::invalid_argument(format!("unknown field: {field_name}"))
        })?;
        let token = analyze_query_term(term, options);
        Ok(vec![Term::new(field_name.to_string(), token)])
    } else {
        let mut terms = Vec::new();
        for (field_name, options) in &schema.fields {
            if !options.indexed {
                continue;
            }
            let token = analyze_query_term(term, options);
            terms.push(Term::new(field_name.clone(), token));
        }
        Ok(terms)
    }
}

fn expand_phrase_query(
    field: Option<&str>,
    terms: &[String],
    schema: &Schema,
) -> crate::Result<Vec<(String, Vec<String>)>> {
    if let Some(field_name) = field {
        let options = schema.get(field_name).ok_or_else(|| {
            crate::Error::invalid_argument(format!("unknown field: {field_name}"))
        })?;
        let analyzed: Vec<String> = terms
            .iter()
            .map(|t| analyze_query_term(t, options))
            .collect();
        Ok(vec![(field_name.to_string(), analyzed)])
    } else {
        let mut out = Vec::new();
        for (field_name, options) in &schema.fields {
            if !options.indexed {
                continue;
            }
            let analyzed: Vec<String> = terms
                .iter()
                .map(|t| analyze_query_term(t, options))
                .collect();
            out.push((field_name.clone(), analyzed));
        }
        Ok(out)
    }
}

fn analyze_query_term(term: &str, options: &FieldOptions) -> String {
    let lowered = term.to_lowercase();
    if options.tokenize && options.stem {
        stem(&lowered)
    } else {
        lowered
    }
}

fn score_terms(
    source: &dyn SegmentSource<Error = crate::Error>,
    scorer: &Bm25Scorer,
    terms: &[Term],
) -> crate::Result<HashMap<DocId, f32>> {
    let mut scores: HashMap<DocId, f32> = HashMap::new();
    for term in terms {
        let postings = source.postings(term)?;
        for posting in postings {
            let s = scorer.score_posting(source, term, &posting);
            *scores.entry(posting.doc_id).or_default() += s;
        }
    }
    Ok(scores)
}

fn score_phrase(
    source: &dyn SegmentSource<Error = crate::Error>,
    scorer: &Bm25Scorer,
    field_terms: &[(String, Vec<String>)],
) -> crate::Result<HashMap<DocId, f32>> {
    let mut scores: HashMap<DocId, f32> = HashMap::new();
    for (field_name, terms) in field_terms {
        if terms.is_empty() {
            continue;
        }

        // Collect postings per term.
        let mut postings_by_term: Vec<Vec<Posting>> = Vec::new();
        for token in terms {
            let term = Term::new(field_name.clone(), token.clone());
            postings_by_term.push(source.postings(&term)?);
        }

        // Intersect doc ids.
        let mut candidates: HashSet<DocId> = postings_by_term[0]
            .iter()
            .map(|p| p.doc_id.clone())
            .collect();
        for list in &postings_by_term[1..] {
            let ids: HashSet<DocId> = list.iter().map(|p| p.doc_id.clone()).collect();
            candidates.retain(|id| ids.contains(id));
        }

        'doc_loop: for doc_id in candidates {
            // Gather positions for each term.
            let mut positions: Vec<Vec<u32>> = Vec::new();
            let mut term_score = 0.0f32;
            for (i, token) in terms.iter().enumerate() {
                let term = Term::new(field_name.clone(), token.clone());
                let list = &postings_by_term[i];
                let Some(posting) = list.iter().find(|p| p.doc_id == doc_id) else {
                    continue 'doc_loop;
                };
                positions.push(posting.positions.clone());
                term_score += scorer.score_posting(source, &term, posting);
            }
            let pos_refs: Vec<&[u32]> = positions.iter().map(|v| v.as_slice()).collect();
            if positions_match_phrase(&pos_refs, 0) {
                *scores.entry(doc_id).or_default() += term_score;
            }
        }
    }
    Ok(scores)
}

fn score_prefix(
    source: &dyn SegmentSource<Error = crate::Error>,
    scorer: &Bm25Scorer,
    field: Option<&str>,
    prefix: &str,
) -> crate::Result<HashMap<DocId, f32>> {
    let mut scores: HashMap<DocId, f32> = HashMap::new();
    let prefix = prefix.to_lowercase();
    let matches = source.prefix_postings(field, &prefix)?;
    for (term, postings) in matches {
        for posting in postings {
            let s = scorer.score_posting(source, &term, &posting);
            *scores.entry(posting.doc_id).or_default() += s;
        }
    }
    Ok(scores)
}

fn score_boolean(
    source: &dyn SegmentSource<Error = crate::Error>,
    scorer: &Bm25Scorer,
    schema: &Schema,
    must: &[Query],
    should: &[Query],
    must_not: &[Query],
) -> crate::Result<HashMap<DocId, f32>> {
    // Evaluate must clauses.
    let mut must_matches: Vec<HashMap<DocId, f32>> = Vec::new();
    for q in must {
        must_matches.push(search_segment(q, schema, source, scorer)?);
    }
    let mut candidates: HashSet<DocId> = if let Some(first) = must_matches.first() {
        let mut candidates = first.keys().cloned().collect::<HashSet<DocId>>();
        for m in must_matches.iter().skip(1) {
            let ids: HashSet<DocId> = m.keys().cloned().collect();
            candidates.retain(|id| ids.contains(id));
        }
        candidates
    } else {
        HashSet::new()
    };

    // Evaluate should clauses.
    let mut should_matches: Vec<HashMap<DocId, f32>> = Vec::new();
    for q in should {
        should_matches.push(search_segment(q, schema, source, scorer)?);
    }
    if !should.is_empty() {
        for m in &should_matches {
            candidates.extend(m.keys().cloned());
        }
    }

    // Evaluate must_not clauses.
    let mut must_not_matches: HashSet<DocId> = HashSet::new();
    for q in must_not {
        let m = search_segment(q, schema, source, scorer)?;
        must_not_matches.extend(m.keys().cloned());
    }

    // A pure must_not query means "all docs except these".
    if must.is_empty() && should.is_empty() {
        let mut all = source.all_doc_ids();
        all.retain(|id| !must_not_matches.contains(id));
        candidates = all.into_iter().collect();
    } else {
        candidates.retain(|id| !must_not_matches.contains(id));
    }

    // No positive clauses and no must_not exclusion means no results.
    if must.is_empty() && should.is_empty() && must_not.is_empty() {
        return Ok(HashMap::new());
    }

    // Compute scores.
    let mut scores: HashMap<DocId, f32> = HashMap::new();
    for doc_id in candidates {
        let mut score = 0.0f32;
        for m in &must_matches {
            if let Some(&s) = m.get(&doc_id) {
                score += s;
            }
        }
        for m in &should_matches {
            if let Some(&s) = m.get(&doc_id) {
                score += s;
            }
        }
        scores.insert(doc_id, score);
    }
    Ok(scores)
}
