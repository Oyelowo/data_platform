use lasso::{Key, Spur, ThreadedRodeo};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(Spur);

impl Symbol {
    pub fn as_str<'a>(&'a self, interner: &'a Interner) -> &'a str {
        interner.resolve(self)
    }

    pub fn as_usize(self) -> usize {
        self.0.into_usize()
    }
}

#[derive(Debug, Clone)]
pub struct Interner {
    rodeo: Arc<ThreadedRodeo<Spur>>,
}

impl Interner {
    pub fn new() -> Self {
        let rodeo = Arc::new(ThreadedRodeo::new());
        Self { rodeo }
    }

    pub fn intern(&self, s: &str) -> Symbol {
        Symbol(self.rodeo.get_or_intern(s))
    }

    pub fn resolve(&self, symbol: &Symbol) -> &str {
        self.rodeo.resolve(&symbol.0)
    }

    pub fn get_or_intern(&self, s: &str) -> Symbol {
        self.intern(s)
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}
