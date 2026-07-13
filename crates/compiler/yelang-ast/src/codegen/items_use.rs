use crate::item::{ModDef, ModKind, Use, UseTree};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Modules / Use ---

impl Codegen for ModDef {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "mod {}", interner.resolve(&self.name.symbol))?;
        match &self.kind {
            ModKind::External => write!(f, ";"),
            ModKind::Inline { items } => {
                writeln!(f, " {{")?;
                for item in items {
                    item.codegen(f, interner)?;
                    writeln!(f)?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Codegen for UseTree {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            UseTree::Simple { path, .. } => path.codegen(f, interner),
            UseTree::Rename { path, alias, .. } => {
                path.codegen(f, interner)?;
                write!(f, " as {}", interner.resolve(&alias.symbol))
            }
            UseTree::Glob { path, .. } => {
                path.codegen(f, interner)?;
                write!(f, "::*")
            }
            UseTree::Nested { prefix, items, .. } => {
                prefix.codegen(f, interner)?;
                write!(f, "::{{")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    item.codegen(f, interner)?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Codegen for Use {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "use ")?;
        self.tree.codegen(f, interner)?;
        write!(f, ";")
    }
}
