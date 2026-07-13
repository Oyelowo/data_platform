use crate::types::{FunctionType, Type, TypeKind, TypeOperator};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Types ---

impl Codegen for Type {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match &self.kind {
            TypeKind::Named(path) => {
                // Generics are in path.segments[].args, so just codegen the path
                path.codegen(f, interner)
            }
            TypeKind::Tuple(types) => {
                write!(f, "(")?;
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    ty.codegen(f, interner)?;
                }
                write!(f, ")")
            }
            TypeKind::Array(ty, size) => {
                write!(f, "[")?;
                ty.codegen(f, interner)?;
                write!(f, "; ")?;
                size.codegen(f, interner)?;
                write!(f, "]")
            }
            TypeKind::Slice(ty) => {
                write!(f, "[")?;
                ty.codegen(f, interner)?;
                write!(f, "]")
            }
            TypeKind::Ref { ty, is_mut } => {
                write!(f, "&")?;
                if *is_mut {
                    write!(f, "mut ")?;
                }
                ty.codegen(f, interner)
            }
            TypeKind::Function(func) => func.codegen(f, interner),
            TypeKind::ForAll { params, ty } => {
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
                ty.codegen(f, interner)
            }
            TypeKind::Never => write!(f, "!"),
            TypeKind::Infer => write!(f, "_"),
            TypeKind::Union(types) => {
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    ty.codegen(f, interner)?;
                }
                Ok(())
            }
            TypeKind::Literal(lit) => lit.codegen(f, interner),
            TypeKind::Structural(fields) => {
                write!(f, "{{")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: ", interner.resolve(&field.name.symbol))?;
                    field.ty.codegen(f, interner)?;
                }
                write!(f, "}}")
            }
            TypeKind::Operator(op) => op.codegen(f, interner),
            TypeKind::ImplTrait(path) => {
                write!(f, "impl ")?;
                path.codegen(f, interner)
            }
            TypeKind::DynTrait(path) => {
                write!(f, "dyn ")?;
                path.codegen(f, interner)
            }
            TypeKind::Error => write!(f, "/* error type */"),
        }
    }
}

impl Codegen for FunctionType {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if self.is_async {
            write!(f, "async ")?;
        }
        write!(f, "fn(")?;
        for (i, param) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            param.codegen(f, interner)?;
        }
        if self.is_variadic {
            write!(f, ", ...")?;
        }
        write!(f, ") -> ")?;
        self.return_type.codegen(f, interner)
    }
}

impl Codegen for TypeOperator {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            TypeOperator::TypeOf(expr) => {
                write!(f, "typeof ")?;
                expr.codegen(f, interner)
            }
            TypeOperator::ReturnType(ty) => {
                write!(f, "ReturnType<")?;
                ty.codegen(f, interner)?;
                write!(f, ">")
            }
            TypeOperator::Parameters(ty) => {
                write!(f, "Parameters<")?;
                ty.codegen(f, interner)?;
                write!(f, ">")
            }
            TypeOperator::Pick(base, keys) => {
                write!(f, "Pick<")?;
                base.codegen(f, interner)?;
                write!(f, ", ")?;
                keys.codegen(f, interner)?;
                write!(f, ">")
            }
            TypeOperator::Omit(base, keys) => {
                write!(f, "Omit<")?;
                base.codegen(f, interner)?;
                write!(f, ", ")?;
                keys.codegen(f, interner)?;
                write!(f, ">")
            }
        }
    }
}
