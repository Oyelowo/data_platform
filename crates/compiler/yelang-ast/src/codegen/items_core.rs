use crate::item::{Item, ItemKind};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Items (core) ---

impl Codegen for Item {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        for attr in &self.attributes {
            attr.codegen(f, interner)?;
        }
        if !self.visibility.is_private() {
            self.visibility.codegen(f, interner)?;
            write!(f, " ")?;
        }
        self.kind.codegen(f, interner)
    }
}

impl Codegen for ItemKind {
    fn codegen(&self, w: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            ItemKind::Module(m) => m.codegen(w, interner),
            ItemKind::Struct(s) => s.codegen(w, interner),
            ItemKind::Enum(e) => e.codegen(w, interner),
            ItemKind::TypeAlias(t) => t.codegen(w, interner),
            ItemKind::Trait(t) => t.codegen(w, interner),
            ItemKind::Fn(f) => f.codegen(w, interner),
            ItemKind::Const(c) => c.codegen(w, interner),
            ItemKind::Static(s) => s.codegen(w, interner),
            ItemKind::Impl(i) => i.codegen(w, interner),
            ItemKind::Use(u) => u.codegen(w, interner),
        }
    }
}
