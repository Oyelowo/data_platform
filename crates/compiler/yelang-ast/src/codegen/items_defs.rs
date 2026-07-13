use crate::item::{
    Const, Enum, FieldDef, FnDef, Static, Struct, StructFields, TypeAlias, VariantDef, VariantKind,
};
use crate::{Codegen, Interner};
use crate::{FnRefType, FnSig};
use std::fmt::{self, Write};

// --- Struct / Enum / Fn / Value Items ---

impl Codegen for FnRefType {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            FnRefType::Type(ty) => ty.codegen(f, interner),
            FnRefType::Default(_) => Ok(()),
        }
    }
}

impl Codegen for FnSig {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "(")?;
        for (i, param) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            param.codegen(f, interner)?;
        }
        write!(f, ")")?;
        if let FnRefType::Type(ty) = &self.return_type {
            write!(f, " -> ")?;
            ty.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for Struct {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "struct {}", interner.resolve(&self.name.symbol))?;
        self.generics.codegen(f, interner)?;
        if let Some(where_clause) = &self.generics.where_clause {
            where_clause.codegen(f, interner)?;
        }
        match &self.fields {
            StructFields::Named(fields) => {
                write!(f, " {{")?;
                for (i, field) in fields.iter().enumerate() {
                    write!(f, " {}: ", interner.resolve(&field.name.symbol))?;
                    field.ty.codegen(f, interner)?;
                    if i < fields.len() - 1 {
                        write!(f, ",")?;
                    }
                }
                write!(f, " }}")
            }
            StructFields::Tuple(types) => {
                write!(f, " (")?;
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    ty.codegen(f, interner)?;
                }
                write!(f, " );")
            }
            StructFields::Unit => write!(f, ";"),
        }
    }
}

impl Codegen for Enum {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "enum {}", interner.resolve(&self.name.symbol))?;
        self.generics.codegen(f, interner)?;
        if let Some(where_clause) = &self.generics.where_clause {
            where_clause.codegen(f, interner)?;
        }
        write!(f, " {{")?;
        for (i, variant) in self.variants.iter().enumerate() {
            variant.codegen(f, interner)?;
            if i < self.variants.len() - 1 {
                write!(f, ",")?;
            }
        }
        write!(f, "}}")
    }
}

impl Codegen for FnDef {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if self.is_const {
            write!(f, "const ")?;
        }
        if self.sig.is_async {
            write!(f, "async ")?;
        }
        write!(f, "fn {}", interner.resolve(&self.name.symbol))?;
        self.generics.codegen(f, interner)?;
        write!(f, "(")?;
        for (i, param) in self.sig.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            param.codegen(f, interner)?;
        }
        write!(f, ")")?;
        if let FnRefType::Type(ty) = &self.sig.return_type {
            write!(f, " -> ")?;
            ty.codegen(f, interner)?;
        }
        if let Some(where_clause) = &self.generics.where_clause {
            write!(f, " ")?;
            where_clause.codegen(f, interner)?;
        }
        write!(f, " ")?;
        self.body.codegen(f, interner)
    }
}

impl Codegen for FieldDef {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}: ", interner.resolve(&self.name.symbol))?;
        self.ty.codegen(f, interner)
    }
}

impl Codegen for VariantDef {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        for attr in &self.attributes {
            attr.codegen(f, interner)?;
            write!(f, " ")?;
        }
        write!(f, "{}", interner.resolve(&self.name.symbol))?;
        match &self.kind {
            VariantKind::Unit => {}
            VariantKind::Tuple(types) => {
                write!(f, "(")?;
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    ty.codegen(f, interner)?;
                }
                write!(f, ")")?;
            }
            VariantKind::Struct(fields) => {
                write!(f, " {{")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    field.codegen(f, interner)?;
                }
                write!(f, "}}")?;
            }
        }
        if let Some(disc) = &self.discriminant {
            write!(f, " = ")?;
            disc.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for Const {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "const {}: ", interner.resolve(&self.name.symbol))?;
        self.ty.codegen(f, interner)?;
        write!(f, " = ")?;
        self.value.codegen(f, interner)?;
        write!(f, ";")
    }
}

impl Codegen for Static {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "static")?;
        if self.mutability {
            write!(f, " mut")?;
        }
        write!(f, " {}: ", interner.resolve(&self.name.symbol))?;
        self.ty.codegen(f, interner)?;
        write!(f, " = ")?;
        self.value.codegen(f, interner)?;
        write!(f, ";")
    }
}

impl Codegen for TypeAlias {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "type {}", interner.resolve(&self.name.symbol))?;
        self.generics.codegen(f, interner)?;
        if let Some(where_clause) = &self.generics.where_clause {
            where_clause.codegen(f, interner)?;
        }
        write!(f, " = ")?;
        self.target.codegen(f, interner)
    }
}
