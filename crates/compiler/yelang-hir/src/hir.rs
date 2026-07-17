//! Core HIR types.
//!
//! This module defines the "shell" structs (`Expr`, `Item`, `Pat`, `Ty`, …)
//! and shared auxiliary types (`Block`, `Stmt`, `Arm`, `FnSig`, …).
//! The `*Kind` enums live in their own sub-modules to keep file sizes
/// reasonable.
pub use yelang_ast::{Ident, Label, Mutability, Visibility};
use yelang_lexer::Span;

pub use crate::hir_body::{Body, Param};
pub use crate::hir_expr::{Expr, ExprKind};
pub use crate::hir_item::{Item, ItemKind};
pub use crate::hir_pat::{Pat, PatKind};
pub use crate::hir_struct::{FieldDef, StructField, VariantData};
pub use crate::hir_ty::{Ty, TyKind};

use crate::hir_ty::Const;
use crate::ids::{BodyId, DefId, HirId};
use crate::res::Res;

/// Re-export commonly-used AST types that contain no unresolved names.
pub type Lit = yelang_lexer::Literal;
pub type BinOp = yelang_ast::BinaryOp;
pub type UnOp = yelang_ast::UnaryOp;

// ---------------------------------------------------------------------------
// Statements and blocks
// ---------------------------------------------------------------------------

/// A block of statements with an optional trailing expression.
#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub expr: Option<Box<Expr>>,
    pub span: Span,
}

/// A statement.
#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

/// Kinds of statements.
#[derive(Debug, Clone)]
pub enum StmtKind {
    /// Expression statement (with or without semicolon).
    Expr { expr: Box<Expr> },
    /// `let` binding.
    Let {
        pat: Pat,
        ty: Option<Ty>,
        init: Option<Box<Expr>>,
    },
    /// Nested item declaration.
    Item { item: Item },
}

// ---------------------------------------------------------------------------
// Match arms and struct fields
// ---------------------------------------------------------------------------

/// A single arm in a `match`.
#[derive(Debug, Clone)]
pub struct Arm {
    pub pat: Pat,
    pub guard: Option<Box<Expr>>,
    pub body: Box<Expr>,
    pub span: Span,
}

/// A field in a struct literal expression.
#[derive(Debug, Clone)]
pub struct FieldExpr {
    pub ident: Ident,
    pub expr: Expr,
    pub span: Span,
}

/// Capture mode for closures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureClause {
    Ref,
    Move,
}

// ---------------------------------------------------------------------------
// Function signatures and generics
// ---------------------------------------------------------------------------

/// Function signature (shared by `fn` items and `fn` pointer types).
#[derive(Debug, Clone)]
pub struct FnSig {
    pub inputs: Vec<Ty>,
    pub output: Ty,
    pub is_async: bool,
    pub is_const: bool,
    pub is_variadic: bool,
    pub abi: Option<String>,
    pub bound_vars: Vec<BoundVarKind>,
}

/// Higher-ranked bound variable (for HRTB).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundVarKind {
    Ty,
    Const,
}

/// Generic parameters and where clause.
#[derive(Debug, Clone)]
pub struct Generics {
    pub params: Vec<GenericParam>,
    pub where_clause: Option<WhereClause>,
    pub span: Span,
}

/// A generic parameter.
#[derive(Debug, Clone)]
pub enum GenericParam {
    Type {
        name: Ident,
        bounds: Vec<TraitBound>,
        default: Option<Ty>,
        span: Span,
    },
    Const {
        name: Ident,
        ty: Ty,
        default: Option<Box<Expr>>,
        span: Span,
    },
}

/// A trait bound in a generic parameter or where clause.
#[derive(Debug, Clone)]
pub struct TraitBound {
    pub path: Res,
    pub span: Span,
}

/// A `where` clause.
#[derive(Debug, Clone)]
pub struct WhereClause {
    pub predicates: Vec<WherePredicate>,
    pub span: Span,
}

/// A single predicate in a `where` clause.
#[derive(Debug, Clone)]
pub enum WherePredicate {
    TraitBound { ty: Ty, bounds: Vec<TraitBound> },
    TypeEq { lhs: Ty, rhs: Ty },
}

// ---------------------------------------------------------------------------
// Enum / Trait / Impl helpers
// ---------------------------------------------------------------------------

/// Definition of an enum (its variants).
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub variants: Vec<VariantDef>,
    pub span: Span,
}

/// A single enum variant.
#[derive(Debug, Clone)]
pub struct VariantDef {
    pub ident: Ident,
    pub data: VariantData,
    pub discriminant: Option<Const>,
    pub span: Span,
}

/// Trait definition.
#[derive(Debug, Clone)]
pub struct Trait {
    pub name: Ident,
    pub generics: Generics,
    pub items: Vec<TraitItem>,
    pub span: Span,
}

/// An item inside a trait definition.
#[derive(Debug, Clone)]
pub struct TraitItem {
    pub ident: Ident,
    pub kind: TraitItemKind,
    pub span: Span,
}

/// Kinds of trait items.
#[derive(Debug, Clone)]
pub enum TraitItemKind {
    Fn {
        sig: FnSig,
        default: Option<BodyId>,
    },
    Const {
        ty: Ty,
        body: Option<BodyId>,
    },
    Type {
        bounds: Vec<TraitBound>,
        default: Option<Ty>,
    },
}

/// An impl block.
#[derive(Debug, Clone)]
pub struct Impl {
    pub generics: Generics,
    pub self_ty: Ty,
    pub of_trait: Option<TraitRef>,
    pub items: Vec<ImplItem>,
    pub span: Span,
}

/// An item inside an impl block.
#[derive(Debug, Clone)]
pub struct ImplItem {
    pub ident: Ident,
    pub kind: ImplItemKind,
    pub span: Span,
    pub defaultness: Defaultness,
}

/// Kinds of impl items.
#[derive(Debug, Clone)]
pub enum ImplItemKind {
    Fn { sig: FnSig, body: BodyId },
    Const { ty: Ty, body: BodyId },
    Type { ty: Ty },
}

/// Reference to a trait in an `impl Trait for Type`.
#[derive(Debug, Clone)]
pub struct TraitRef {
    pub path: Res,
    pub span: Span,
}

/// A path in a `use` item.
#[derive(Debug, Clone)]
pub struct UsePath {
    pub res: Res,
    pub span: Span,
}

/// Kinds of `use` imports.
#[derive(Debug, Clone)]
pub enum UseKind {
    Single,
    Glob,
    Nested { items: Vec<UsePath> },
}

/// Foreign item in an `extern` block.
#[derive(Debug, Clone)]
pub struct ForeignItem {
    pub ident: Ident,
    pub kind: ForeignItemKind,
    pub span: Span,
}

/// Kinds of foreign items.
#[derive(Debug, Clone)]
pub enum ForeignItemKind {
    Fn { sig: FnSig },
    Static { ty: Ty, mutability: Mutability },
    Type,
}

/// Whether an impl item is marked `default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defaultness {
    Default,
    Final,
}
