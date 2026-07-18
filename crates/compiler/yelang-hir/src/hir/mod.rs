//! HIR node definitions and visitor.
//!
//! This module contains the core AST-like structures of the High-level
//! Intermediate Representation: expressions, patterns, types, items, bodies,
//! algebraic data types, and the visitor that traverses them.

pub mod adt;
pub mod body;
pub mod core;
pub mod expr;
pub mod item;
pub mod pat;
pub mod ty;

// Re-export the most commonly used HIR types at the `hir` level so callers
// can write `crate::hir::Expr`, `crate::hir::Ty`, etc.
pub use adt::{FieldDef, StructField, VariantData};
pub use body::{Body, Param};
pub use core::{
    Arm, BinderParam, Block, BoundVarKind, CaptureClause, Defaultness, EnumDef, FieldExpr, FnSig,
    ForeignItem, ForeignItemKind, GenericParam, Generics, Impl, ImplItem, ImplItemKind,
    ImplPolarity, Item, ItemKind, Stmt, Trait, TraitBound, TraitItem, TraitItemKind, TraitRef,
    UseKind, UsePath, VariantDef, WhereClause, WherePredicate,
};
pub use expr::{ComprehensionKind, DocumentProjection, Expr, GeneratorKind};
pub use pat::{BindingMode, FieldPat, Pat};
pub use ty::{AnonField, Const, ConstKind, GenericArg, Ty, UtilityKind};
pub use crate::visit::visitor::{walk_crate, Visitor};
