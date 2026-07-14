use std::sync::atomic::{AtomicU32, Ordering};

/// A unique identifier for a single macro expansion.
///
/// Every macro invocation receives a fresh `ExpnId` during expansion.
/// `ExpnId::root()` represents the original source code (not expanded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExpnId(u32);

impl ExpnId {
    /// The root expansion — source code that was not produced by a macro.
    pub const ROOT: Self = Self(0);

    /// Allocate a fresh `ExpnId`.
    pub fn fresh() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

/// A syntax context tracks the chain of macro expansions that produced a token.
///
/// In a fully hygienic system, each identifier carries a `SyntaxContext` that
/// determines which scope it can resolve in. For the MVP, we keep a simple
/// context ID that records the expansion chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SyntaxContext(u32);

impl SyntaxContext {
    /// The root context — identifiers from the original source.
    pub const ROOT: Self = Self(0);

    /// Create a new context nested inside the given expansion.
    pub fn apply_mark(self, _expn: ExpnId) -> Self {
        // For the MVP, we simply allocate a fresh context ID.
        // A full implementation would chain contexts for proper hygiene.
        static COUNTER: AtomicU32 = AtomicU32::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

/// Global hygiene data for the current compilation session.
///
/// In rustc, this is a global singleton. For yelang, we keep it simple
/// and thread-local until the compiler is parallelized.
#[derive(Debug, Default)]
pub struct HygieneData {
    // Future: store per-ExpnId metadata (call site, definition site, etc.)
}

impl HygieneData {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expn_id_fresh_increments() {
        let a = ExpnId::fresh();
        let b = ExpnId::fresh();
        assert!(b.0 > a.0, "ExpnId should increment");
    }

    #[test]
    fn syntax_context_apply_mark_produces_distinct() {
        let ctx1 = SyntaxContext::ROOT.apply_mark(ExpnId::fresh());
        let ctx2 = SyntaxContext::ROOT.apply_mark(ExpnId::fresh());
        assert_ne!(ctx1, ctx2, "Different marks should produce different contexts");
    }
}
