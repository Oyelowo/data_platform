use crate::FnRefType;
use crate::item::{
    AssociatedConst, AssociatedType, Impl, ImplItem, ImplItemKind, Trait, TraitItem, TraitItemKind,
};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Trait / Impl Items ---

impl Codegen for AssociatedType {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "type {}", interner.resolve(&self.name.symbol))?;
        self.generics.codegen(f, interner)?;
        if !self.bounds.is_empty() {
            write!(f, ": ")?;
            for (i, bound) in self.bounds.iter().enumerate() {
                if i > 0 {
                    write!(f, " + ")?;
                }
                bound.codegen(f, interner)?;
            }
        }
        if let Some(default) = &self.default {
            write!(f, " = ")?;
            default.codegen(f, interner)?;
        }
        if let Some(where_clause) = &self.generics.where_clause {
            write!(f, " ")?;
            where_clause.codegen(f, interner)?;
        }
        write!(f, ";")
    }
}

impl Codegen for AssociatedConst {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "const {}: ", interner.resolve(&self.name.symbol))?;
        self.ty.codegen(f, interner)?;
        if let Some(value) = &self.value {
            write!(f, " = ")?;
            value.codegen(f, interner)?;
        }
        write!(f, ";")
    }
}

impl Codegen for Trait {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "trait {}", interner.resolve(&self.name.symbol))?;
        self.generics.codegen(f, interner)?;
        if let Some(where_clause) = &self.generics.where_clause {
            where_clause.codegen(f, interner)?;
        }
        if !self.super_traits.is_empty() {
            write!(f, ": ")?;
            for (i, bound) in self.super_traits.iter().enumerate() {
                if i > 0 {
                    write!(f, " + ")?;
                }
                bound.codegen(f, interner)?;
            }
        }
        writeln!(f, " {{")?;
        for item in &self.items {
            item.codegen(f, interner)?;
            writeln!(f)?;
        }
        write!(f, "}}")
    }
}

impl Codegen for TraitItem {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        for attr in &self.attributes {
            attr.codegen(f, interner)?;
            writeln!(f)?;
        }
        self.item.codegen(f, interner)
    }
}

impl Codegen for TraitItemKind {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            TraitItemKind::Method(m) => {
                if m.is_const {
                    write!(f, "const ")?;
                }
                if m.sig.is_async {
                    write!(f, "async ")?;
                }
                write!(f, "fn {}", interner.resolve(&m.segment.symbol))?;
                m.generics.codegen(f, interner)?;
                write!(f, "(")?;
                for (i, param) in m.sig.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    param.codegen(f, interner)?;
                }
                write!(f, ")")?;
                if let FnRefType::Type(ty) = &m.sig.return_type {
                    write!(f, " -> ")?;
                    ty.codegen(f, interner)?;
                }
                write!(f, ";")
            }
            TraitItemKind::AssociatedType(t) => t.codegen(f, interner),
            TraitItemKind::Constant(c) => c.codegen(f, interner),
        }
    }
}

impl Codegen for Impl {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        for attr in &self.attributes {
            attr.codegen(f, interner)?;
            writeln!(f)?;
        }
        if matches!(self.defaultness, crate::item::Defaultness::Default) {
            write!(f, "default ")?;
        }
        write!(f, "impl ")?;
        self.generics.codegen(f, interner)?;
        if let Some(where_clause) = &self.generics.where_clause {
            where_clause.codegen(f, interner)?;
        }
        if let Some(trait_path) = &self.trait_impl {
            write!(f, " ")?;
            trait_path.codegen(f, interner)?;
            write!(f, " for ")?;
        }
        self.self_ty.codegen(f, interner)?;
        writeln!(f, " {{")?;
        for item in &self.items {
            item.codegen(f, interner)?;
            writeln!(f)?;
        }
        write!(f, "}}")
    }
}

impl Codegen for ImplItem {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        for attr in &self.attributes {
            attr.codegen(f, interner)?;
            writeln!(f)?;
        }
        self.visibility.codegen(f, interner)?;
        if matches!(self.defaultness, crate::item::Defaultness::Default) {
            write!(f, "default ")?;
        }
        self.item.codegen(f, interner)
    }
}

impl Codegen for ImplItemKind {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            ImplItemKind::Method(m) => m.codegen(f, interner),
            ImplItemKind::AssociatedType(t) => {
                write!(f, "type {}", interner.resolve(&t.name.symbol))?;
                t.generics.codegen(f, interner)?;
                write!(f, " = ")?;
                t.ty.codegen(f, interner)?;
                if let Some(where_clause) = &t.generics.where_clause {
                    write!(f, " ")?;
                    where_clause.codegen(f, interner)?;
                }
                write!(f, ";")
            }
            ImplItemKind::Constant(c) => c.codegen(f, interner),
        }
    }
}
