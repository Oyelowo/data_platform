use crate::Ident;
use crate::item::{
    Attribute, AttributeArgs, ConstParam, GenericParam, Generics, NamedArg, Param,
    ParenthesizedArgs, TraitBound, TypeParam, Visibility, WhereClause, WherePredicate,
};
use crate::{Codegen, Interner, Mutability, Path, PatternKind, Type, TypeKind};
use std::fmt::{self, Write};
use yelang_interner::Symbol;

// --- Visibility / Attributes / Generics ---

impl Codegen for Visibility {
    fn codegen(&self, f: &mut dyn Write, _interner: &Interner) -> fmt::Result {
        match self {
            Visibility::Private => Ok(()),
            Visibility::Public(_) => write!(f, "pub "),
            Visibility::PublicCrate(_) => write!(f, "pub(crate) "),
            Visibility::PublicSuper(_) => write!(f, "pub(super) "),
            Visibility::PublicSelf(_) => write!(f, "pub(self) "),
            Visibility::PublicIn { path, .. } => {
                write!(f, "pub(in ")?;
                path.codegen(f, _interner)?;
                write!(f, ") ")
            }
        }
    }
}

impl Codegen for Attribute {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "@")?;
        if self.is_absolute {
            write!(f, "::")?;
        }
        for (i, ident) in self.path.iter().enumerate() {
            if i > 0 {
                write!(f, "::")?;
            }
            write!(f, "{}", interner.resolve(&ident.symbol))?;
        }
        match &self.args {
            AttributeArgs::Empty => {}
            AttributeArgs::Positional(exprs) => {
                write!(f, "(")?;
                for (i, expr) in exprs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    expr.codegen(f, interner)?;
                }
                write!(f, ")")?;
            }
            AttributeArgs::Named(named) => {
                write!(f, "(")?;
                for (i, arg) in named.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    arg.codegen(f, interner)?;
                }
                write!(f, ")")?;
            }
        }
        Ok(())
    }
}

impl Codegen for NamedArg {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "{} = ", interner.resolve(&self.name.symbol))?;
        self.value.codegen(f, interner)
    }
}

impl Codegen for Generics {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if !self.params.is_empty() {
            write!(f, "<")?;
            for (i, param) in self.params.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                param.codegen(f, interner)?;
            }
            write!(f, ">")?;
        }
        Ok(())
    }
}

impl Codegen for ParenthesizedArgs {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "(")?;
        for (i, ty) in self.ins.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            ty.codegen(f, interner)?;
        }
        write!(f, ")")?;
        if let Some(out) = &self.out {
            write!(f, " -> ")?;
            out.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for GenericParam {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            GenericParam::Type(t) => t.codegen(f, interner),
            GenericParam::Const(c) => c.codegen(f, interner),
        }
    }
}

impl Codegen for TypeParam {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}", interner.resolve(&self.name.symbol))?;
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
        Ok(())
    }
}

impl Codegen for ConstParam {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "const {}: ", interner.resolve(&self.name.symbol))?;
        self.ty.codegen(f, interner)?;
        if let Some(default) = &self.default {
            write!(f, " = ")?;
            default.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for WhereClause {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "where ")?;
        for (i, pred) in self.predicates.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            pred.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for WherePredicate {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            WherePredicate::ForAll {
                params, predicate, ..
            } => {
                write!(f, "for<")?;
                for (i, p) in params.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match p {
                        crate::item::TypeBinderParam::Type(tp) => {
                            write!(f, "{}", interner.resolve(&tp.name.symbol))?;
                            if !tp.bounds.is_empty() {
                                write!(f, ": ")?;
                                for (j, bound) in tp.bounds.iter().enumerate() {
                                    if j > 0 {
                                        write!(f, " + ")?;
                                    }
                                    bound.codegen(f, interner)?;
                                }
                            }
                        }
                        crate::item::TypeBinderParam::Const(c) => {
                            write!(f, "const {}: ", interner.resolve(&c.name.symbol))?;
                            c.ty.codegen(f, interner)?;
                        }
                    }
                }
                write!(f, "> ")?;
                predicate.codegen(f, interner)
            }
            WherePredicate::TraitBound { ty, bounds } => {
                ty.codegen(f, interner)?;
                write!(f, ": ")?;
                for (i, bound) in bounds.iter().enumerate() {
                    if i > 0 {
                        write!(f, " + ")?;
                    }
                    bound.codegen(f, interner)?;
                }
                Ok(())
            }
            WherePredicate::TypeEq { lhs, rhs } => {
                lhs.codegen(f, interner)?;
                write!(f, " = ")?;
                rhs.codegen(f, interner)
            }
        }
    }
}

impl Codegen for TraitBound {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(b) = &self.binder {
            write!(f, "for<")?;
            for (i, p) in b.params.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match p {
                    crate::item::TypeBinderParam::Type(tp) => {
                        write!(f, "{}", interner.resolve(&tp.name.symbol))?;
                        if !tp.bounds.is_empty() {
                            write!(f, ": ")?;
                            for (j, bound) in tp.bounds.iter().enumerate() {
                                if j > 0 {
                                    write!(f, " + ")?;
                                }
                                bound.codegen(f, interner)?;
                            }
                        }
                    }
                    crate::item::TypeBinderParam::Const(c) => {
                        write!(f, "const {}: ", interner.resolve(&c.name.symbol))?;
                        c.ty.codegen(f, interner)?;
                    }
                }
            }
            write!(f, "> ")?;
        }
        self.path.codegen(f, interner)
    }
}

impl Codegen for Param {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(self_param) = canonical_self_param(self, interner) {
            return write!(f, "{self_param}");
        }

        self.pattern.codegen(f, interner)?;
        write!(f, ": ")?;
        self.ty.codegen(f, interner)
    }
}

fn canonical_self_param<'a>(param: &Param, interner: &'a Interner) -> Option<&'static str> {
    let PatternKind::Binding {
        name,
        mutability,
        subpattern: None,
    } = &param.pattern.pattern
    else {
        return None;
    };

    if interner.resolve(&name.symbol) != "self" {
        return None;
    }

    match &param.ty.kind {
        TypeKind::Named(path) if is_self_path(path, interner) => Some(match mutability {
            Mutability::Immutable => "self",
            Mutability::Mutable => "mut self",
        }),
        TypeKind::Ref { ty, is_mut } if is_self_type(ty, interner) => {
            Some(if *is_mut { "&mut self" } else { "&self" })
        }
        _ => None,
    }
}

fn is_self_type(ty: &Type, interner: &Interner) -> bool {
    matches!(&ty.kind, TypeKind::Named(path) if is_self_path(path, interner))
}

fn is_self_path(path: &Path, interner: &Interner) -> bool {
    !path.is_absolute
        && path.qself.is_none()
        && path.segments.len() == 1
        && interner.resolve(&path.segments[0].ident.symbol) == "Self"
}

impl Codegen for Ident {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}", interner.resolve(&self.symbol))
    }
}

impl Codegen for Symbol {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}", interner.resolve(self))
    }
}
