use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::id::{
    ExpnArena, ExpnData, ExpnId, ExpnKind, SyntaxContextData, SyntaxContextId, Transparency,
};

/// Global hygiene data for the current compilation session.
///
/// In a fully parallel compiler this would be thread-local or sharded. For now
/// it is stored behind a mutex so it can be accessed from any `Span`.
#[derive(Debug)]
pub struct HygieneData {
    expn_arena: Mutex<ExpnArena>,
    syntax_contexts: Mutex<HashMap<SyntaxContextId, SyntaxContextData>>,
    /// Canonicalization map so that identical `(parent, expansion, transparency)`
    /// triples reuse the same syntax context. Without this, repeated macro
    /// expansions create exponentially many equivalent contexts.
    context_dedup: Mutex<HashMap<(SyntaxContextId, ExpnId, Transparency), SyntaxContextId>>,
    next_syntax_context_id: AtomicU32,
    root_expn: ExpnId,
    root_syntax_context: SyntaxContextId,
}

impl HygieneData {
    pub fn new() -> Self {
        let mut expn_arena = ExpnArena::new();
        let root_expn_key = expn_arena.insert(ExpnData {
            parent: ExpnId::default(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Root,
            desc: "root".to_string(),
        });
        let root_expn = ExpnId::from_arena_key(root_expn_key);

        let root_syntax_context = SyntaxContextId::new(1);
        let mut syntax_contexts = HashMap::new();
        syntax_contexts.insert(root_syntax_context, SyntaxContextData::root());

        Self {
            expn_arena: Mutex::new(expn_arena),
            syntax_contexts: Mutex::new(syntax_contexts),
            context_dedup: Mutex::new(HashMap::new()),
            next_syntax_context_id: AtomicU32::new(2),
            root_expn,
            root_syntax_context,
        }
    }

    pub fn root_expn(&self) -> ExpnId {
        self.root_expn
    }

    pub fn root_syntax_context(&self) -> SyntaxContextId {
        self.root_syntax_context
    }

    pub fn fresh_expn(&self, data: ExpnData) -> ExpnId {
        ExpnId::from_arena_key(self.expn_arena.lock().unwrap().insert(data))
    }

    pub fn expn_data(&self, id: ExpnId) -> Option<ExpnData> {
        self.expn_arena
            .lock()
            .unwrap()
            .get(id.as_arena_key())
            .cloned()
    }

    /// Insert a new expansion and return its allocated `ExpnId`.
    pub fn insert_expn(&self, data: ExpnData) -> ExpnId {
        ExpnId::from_arena_key(self.expn_arena.lock().unwrap().insert(data))
    }

    /// Update an existing expansion in place.
    pub fn update_expn<F: FnOnce(&mut ExpnData)>(&self, id: ExpnId, f: F) {
        if let Some(data) = self.expn_arena.lock().unwrap().get_mut(id.as_arena_key()) {
            f(data);
        }
    }

    pub fn apply_mark(
        &self,
        parent: SyntaxContextId,
        expn: ExpnId,
        transparency: Transparency,
    ) -> SyntaxContextId {
        let key = (parent, expn, transparency);
        let mut dedup = self.context_dedup.lock().unwrap();
        if let Some(&id) = dedup.get(&key) {
            return id;
        }

        let data = SyntaxContextData {
            parent: Some(parent),
            outer_expn: Some(expn),
            transparency,
        };
        let id = self.fresh_syntax_context_id();
        self.syntax_contexts.lock().unwrap().insert(id, data);
        dedup.insert(key, id);
        id
    }

    /// Create a syntax context with a specific ID and data.
    ///
    /// Used when deserializing hygiene data from the proc-macro server. If the
    /// ID already exists, its data is overwritten.
    pub fn insert_syntax_context(&self, id: SyntaxContextId, data: SyntaxContextData) {
        let mut contexts = self.syntax_contexts.lock().unwrap();
        contexts.insert(id, data.clone());
        drop(contexts);

        // Keep the canonicalization map in sync when contexts are materialized
        // from an external source (e.g. the proc-macro server).
        if let (Some(parent), Some(outer_expn)) = (data.parent, data.outer_expn) {
            self.context_dedup
                .lock()
                .unwrap()
                .insert((parent, outer_expn, data.transparency), id);
        }

        let current_next = self.next_syntax_context_id.load(Ordering::SeqCst);
        let needed = id.raw().saturating_add(1);
        if needed > current_next {
            self.next_syntax_context_id.store(needed, Ordering::SeqCst);
        }
    }

    pub fn syntax_context_data(&self, id: SyntaxContextId) -> Option<SyntaxContextData> {
        self.syntax_contexts.lock().unwrap().get(&id).cloned()
    }

    fn fresh_syntax_context_id(&self) -> SyntaxContextId {
        let raw = self.next_syntax_context_id.fetch_add(1, Ordering::SeqCst);
        SyntaxContextId::new(raw)
    }
}

impl Default for HygieneData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hygiene_data_has_root_context() {
        let data = HygieneData::new();
        assert!(
            data.syntax_context_data(data.root_syntax_context())
                .is_some()
        );
    }

    #[test]
    fn apply_mark_creates_distinct_context() {
        let data = HygieneData::new();
        let expn = data.fresh_expn(ExpnData {
            parent: data.root_expn(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Macro,
            desc: "test".to_string(),
        });
        let ctx = data.apply_mark(data.root_syntax_context(), expn, Transparency::Opaque);
        assert_ne!(ctx, data.root_syntax_context());
    }

    #[test]
    fn apply_mark_reuses_equivalent_contexts() {
        let data = HygieneData::new();
        let expn = data.fresh_expn(ExpnData {
            parent: data.root_expn(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Macro,
            desc: "test".to_string(),
        });
        let root = data.root_syntax_context();
        let ctx1 = data.apply_mark(root, expn, Transparency::Opaque);
        let ctx2 = data.apply_mark(root, expn, Transparency::Opaque);
        assert_eq!(ctx1, ctx2, "identical marks should be deduplicated");
    }

    #[test]
    fn apply_mark_distinct_for_different_transparency() {
        let data = HygieneData::new();
        let expn = data.fresh_expn(ExpnData {
            parent: data.root_expn(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Macro,
            desc: "test".to_string(),
        });
        let root = data.root_syntax_context();
        let opaque = data.apply_mark(root, expn, Transparency::Opaque);
        let transparent = data.apply_mark(root, expn, Transparency::Transparent);
        assert_ne!(opaque, transparent);
    }

    #[test]
    fn insert_expn_and_update_parent_round_trip() {
        let data = HygieneData::new();
        let child = data.insert_expn(ExpnData {
            parent: data.root_expn(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Macro,
            desc: "child".to_string(),
        });
        let parent = data.insert_expn(ExpnData {
            parent: data.root_expn(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Macro,
            desc: "parent".to_string(),
        });
        data.update_expn(child, |e| e.parent = parent);
        assert_eq!(data.expn_data(child).unwrap().parent, parent);
    }
}
