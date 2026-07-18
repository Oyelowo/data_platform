//! Core HIR types.
//!
//! This module defines the "shell" structs (`Item`, `Block`, `Arm`, `FnSig`, …)
//! and shared auxiliary types.  The expression/pattern/type/statement enums
//! live in their own sub-modules and are referenced here by ID.
pub use yelang_ast::{Attribute, Ident, Label, Mutability, Visibility};
use yelang_lexer::Span;

pub use crate::hir::body::{Body, Param};
pub use crate::hir::expr::Expr;
pub use crate::hir::item::{Item, ItemKind};
pub use crate::hir::pat::Pat;
pub use crate::hir::adt::{FieldDef, StructField, VariantData};
pub use crate::hir::ty::Ty;

use crate::ids::{BodyId, DefId, ExprId, PatId, StmtId, HirTyId};
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
    pub stmts: Vec<StmtId>,
    pub expr: Option<ExprId>,
    pub span: Span,
}

/// Kinds of statements.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// Expression statement (with or without semicolon).
    Expr { expr: ExprId },
    /// `let` binding.
    Let {
        pat: PatId,
        ty: Option<HirTyId>,
        init: Option<ExprId>,
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
    pub pat: PatId,
    pub guard: Option<ExprId>,
    pub body: ExprId,
    pub span: Span,
}

/// A field in a struct literal expression.
#[derive(Debug, Clone)]
pub struct FieldExpr {
    pub ident: Ident,
    pub expr: ExprId,
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
    pub inputs: Vec<HirTyId>,
    pub output: HirTyId,
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

/// A generic parameter declared on an item (function, struct, trait, impl, ...).
#[derive(Debug, Clone)]
pub enum GenericParam {
    Type {
        def_id: yelang_arena::DefId,
        name: Ident,
        bounds: Vec<TraitBound>,
        default: Option<HirTyId>,
        span: Span,
    },
    Const {
        def_id: yelang_arena::DefId,
        name: Ident,
        ty: HirTyId,
        default: Option<ExprId>,
        span: Span,
    },
}

/// A bound variable introduced by a higher-ranked type binder (`for<T>`).
/// Unlike item-level `GenericParam`s, binder variables are not definitions and
/// therefore do not carry a `DefId`.
#[derive(Debug, Clone)]
pub enum BinderParam {
    Type {
        name: Ident,
        bounds: Vec<TraitBound>,
        span: Span,
    },
    Const {
        name: Ident,
        ty: HirTyId,
        span: Span,
    },
}

/// A trait bound in a generic parameter or where clause.
#[derive(Debug, Clone)]
pub struct TraitBound {
    pub path: Res,
    /// Generic arguments on the trait path, e.g. `U` in `T: Foo<U>`.
    pub args: Vec<crate::hir::ty::GenericArg>,
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
    TraitBound { ty: HirTyId, bounds: Vec<TraitBound> },
    TypeEq { lhs: HirTyId, rhs: HirTyId },
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
    pub def_id: DefId,
    pub ident: Ident,
    pub data: VariantData,
    pub discriminant: Option<crate::hir::ty::Const>,
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

/// Trait definition.
#[derive(Debug, Clone)]
pub struct Trait {
    pub name: Ident,
    pub generics: Generics,
    pub super_traits: Vec<TraitRef>,
    pub items: Vec<TraitItem>,
    pub span: Span,
}

/// An item inside a trait definition.
#[derive(Debug, Clone)]
pub struct TraitItem {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: TraitItemKind,
    pub attrs: Vec<Attribute>,
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
        ty: HirTyId,
        body: Option<BodyId>,
    },
    Type {
        bounds: Vec<TraitBound>,
        default: Option<HirTyId>,
    },
}

/// An impl block.
#[derive(Debug, Clone)]
pub struct Impl {
    pub def_id: DefId,
    pub generics: Generics,
    pub self_ty: HirTyId,
    pub of_trait: Option<TraitRef>,
    pub items: Vec<ImplItem>,
    pub polarity: ImplPolarity,
    pub span: Span,
}

/// Polarity of an impl block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplPolarity {
    /// `impl Trait for Type`
    Positive,
    /// `impl !Trait for Type`
    Negative,
}

/// An item inside an impl block.
#[derive(Debug, Clone)]
pub struct ImplItem {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: ImplItemKind,
    pub attrs: Vec<Attribute>,
    pub span: Span,
    pub defaultness: Defaultness,
}

/// Kinds of impl items.
#[derive(Debug, Clone)]
pub enum ImplItemKind {
    Fn { sig: FnSig, body: BodyId },
    Const { ty: HirTyId, body: BodyId },
    Type { ty: HirTyId },
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
    /// Renamed identifier for `use path as name`.
    pub rename: Option<Ident>,
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
    Static { ty: HirTyId, mutability: Mutability },
    Type,
}

/// Whether an impl item is marked `default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defaultness {
    Default,
    Final,
}
