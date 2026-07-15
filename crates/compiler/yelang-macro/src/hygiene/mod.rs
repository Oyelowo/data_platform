use std::sync::Mutex;

use yelang_util::{
    ExpnArena, ExpnData, ExpnId, ExpnKind, SyntaxContextArena, SyntaxContextData, SyntaxContextId,
    Transparency,
};

/// Global hygiene data for the current compilation session.
///
/// In a fully parallel compiler this would be thread-local or sharded. For now
/// it is stored behind a mutex so it can be accessed from any `Span`.
#[derive(Debug)]
pub struct HygieneData {
    expn_arena: Mutex<ExpnArena>,
    syntax_context_arena: Mutex<SyntaxContextArena>,
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

        let mut syntax_context_arena = SyntaxContextArena::new();
        let root_ctx_key = syntax_context_arena.insert(SyntaxContextData::root());
        let root_syntax_context = SyntaxContextId::from_arena_key(root_ctx_key);

        Self {
            expn_arena: Mutex::new(expn_arena),
            syntax_context_arena: Mutex::new(syntax_context_arena),
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

    pub fn apply_mark(
        &self,
        parent: SyntaxContextId,
        expn: ExpnId,
        transparency: Transparency,
    ) -> SyntaxContextId {
        let data = SyntaxContextData {
            parent: Some(parent),
            outer_expn: Some(expn),
            transparency,
        };
        SyntaxContextId::from_arena_key(self.syntax_context_arena.lock().unwrap().insert(data))
    }

    pub fn syntax_context_data(&self, id: SyntaxContextId) -> Option<SyntaxContextData> {
        self.syntax_context_arena
            .lock()
            .unwrap()
            .get(id.as_arena_key())
            .cloned()
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
}
