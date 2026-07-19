# Specification: yelang-hir

## Overview
High-level Intermediate Representation (HIR) for Yelang. Lowered from AST after name resolution. All names resolved to DefIds, syntax sugar desugared.

## Dependencies
- `yelang-ast` (path: `../yelang-ast`)
- `yelang-lexer` (path: `../yelang-lexer`) — for Span, FileId
- `yelang-interner` (path: `../yelang-interner`) — for Symbol
- `yelang-arena` (path: `../yelang-arena`) — for Arena, ArenaMap, FxHashMap, DefId, HirId, BodyId
- `yelang-resolve` (path: `../yelang-resolve`) — for ResolvedCrate, Resolution
- `thiserror` (workspace)

## CRITICAL RULES
1. **Named struct variants ONLY**: Every enum variant with >1 field MUST be a named struct variant.
2. **mod.rs style**: Every module directory has a `mod.rs` file.
3. **Max file size**: ~500 lines. Split by concern.
4. **No unwrap/expect in library code**: Use `Result` or `Option`. Only `unwrap` in tests.
5. **Error types**: Use `thiserror` for all error enums.
6. **Use yelang-arena types**: `DefId`, `HirId`, `BodyId`, `Arena`, `ArenaMap`, `FxHashMap`.
7. **HIR has no unresolved names**: Every path is a `Res` (resolved path), every variable is a `HirId` or `DefId`.

## Module Structure

```
src/
  lib.rs              — Public exports, Crate root
  ids.rs              — HirId, BodyId, LocalId (or re-export from yelang-arena)
  res.rs              — Res: how a path was resolved (Def, Local, PrimTy, etc.)
  hir.rs              — Core HIR types (Item, Expr, Stmt, Ty, Pat, Body)
  hir_expr.rs         — ExprKind enum (named struct variants)
  hir_item.rs         — ItemKind enum (named struct variants)
  hir_ty.rs           — TyKind enum (named struct variants)
  hir_pat.rs          — PatKind enum (named struct variants)
  hir_body.rs         — Body, Param, local variables
  hir_struct.rs       — VariantData, FieldDef, StructField
  crate.rs            — Crate struct with maps (out-of-band storage like rustc)
  lowering.rs         — LoweringContext, main lowering entry point
  lowering_expr.rs    — Expression lowering (AST Expr -> HIR Expr)
  lowering_item.rs    — Item lowering (AST Item -> HIR Item)
  lowering_ty.rs      — Type lowering (AST Ty -> HIR Ty)
  lowering_pat.rs     — Pattern lowering (AST Pat -> HIR Pat)
  lowering_body.rs    — Body lowering (AST Block/Expr -> HIR Body)
  lowering_err.rs     — LoweringError enum
  map.rs              — HIR map: id -> node lookup (like rustc's hir::map)
  visitor.rs          — HIR visitor trait (walk_*, visit_* methods)
  tests/
    mod.rs
    lowering.rs       — Lowering correctness tests
    desugaring.rs     — Desugaring tests (for, while, ?, async)
    visitor.rs        — Visitor tests
```

## Core Data Structures

### Res (src/res.rs)
How a path was resolved. Similar to rustc's `Res`.
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Res {
    Def { def_id: DefId },           // Resolved to a definition
    Local { hir_id: HirId },         // Resolved to a local variable
    PrimTy { ty: PrimTy },           // Primitive type (i32, bool, etc.)
    SelfTy { def_id: DefId },        // `Self` in impl/trait
    SelfVal { def_id: DefId },       // `self` parameter
    Err,                            // Error recovery
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimTy {
    Int(IntTy),   // i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
    Float(FloatTy), // f32, f64
    Bool,
    Char,
    Str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntTy { I8, I16, I32, I64, I128, Isize, U8, U16, U32, U64, U128, Usize }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatTy { F32, F64 }
```

### Crate (src/crate.rs)
Out-of-band storage like rustc. Items are stored in maps, not inline in the tree.
```rust
#[derive(Debug, Clone)]
pub struct Crate {
    pub root_module: DefId,
    // All items keyed by DefId
    pub items: FxHashMap<DefId, Item>,
    // All bodies keyed by BodyId
    pub bodies: FxHashMap<BodyId, Body>,
    // Trait definitions
    pub traits: FxHashMap<DefId, Trait>,
    // Impl blocks
    pub impls: Vec<Impl>,
    // Foreign items (extern blocks)
    pub foreign_items: FxHashMap<DefId, ForeignItem>,
}
```

### Item (src/hir_item.rs)
```rust
#[derive(Debug, Clone)]
pub struct Item {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: ItemKind,
    pub vis: Visibility,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Fn { sig: FnSig, body: BodyId, generics: Generics },
    Struct { data: VariantData, generics: Generics },
    Enum { def: EnumDef, generics: Generics },
    Union { data: VariantData, generics: Generics },
    Trait { items: Vec<TraitItem>, generics: Generics },
    Impl { items: Vec<ImplItem>, generics: Generics, self_ty: Ty, of_trait: Option<TraitRef> },
    TyAlias { ty: Ty, generics: Generics },
    Const { ty: Ty, body: BodyId },
    Static { ty: Ty, mutability: Mutability, body: BodyId },
    Mod { items: Vec<DefId> },
    Use { path: UsePath, kind: UseKind },
    Macro { def: MacroDef },
}
```

### Expr (src/hir_expr.rs)
All syntax sugar desugared. No `for`, `while`, `?`, `async` in ExprKind.
```rust
#[derive(Debug, Clone)]
pub struct Expr {
    pub hir_id: HirId,
    pub kind: ExprKind,
    pub span: Span,
    pub ty: Ty,  // After type checking, filled in. Lowering leaves this as TyKind::Infer.
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Lit { lit: Lit },
    Path { res: Res },
    Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnOp, expr: Box<Expr> },
    Call { func: Box<Expr>, args: Vec<Expr> },
    MethodCall { receiver: Box<Expr>, method: Ident, args: Vec<Expr>, trait_def_id: Option<DefId> },
    Field { expr: Box<Expr>, field: Ident },
    Index { expr: Box<Expr>, index: Box<Expr> },
    Assign { left: Box<Expr>, right: Box<Expr> },
    Block { block: Block },
    Loop { block: Block, label: Option<Label> },
    Break { label: Option<Label>, expr: Option<Box<Expr>> },
    Continue { label: Option<Label> },
    Return { expr: Option<Box<Expr>> },
    Match { expr: Box<Expr>, arms: Vec<Arm> },
    If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Option<Box<Expr>> },
    Closure { params: Vec<Param>, body: BodyId, capture_clause: CaptureClause },
    Struct { path: Res, fields: Vec<FieldExpr>, rest: Option<Box<Expr>> },
    Tuple { exprs: Vec<Expr> },
    Array { exprs: Vec<Expr> },
    Cast { expr: Box<Expr>, ty: Ty },
    Err,
}
```

### Ty (src/hir_ty.rs)
Support for anonymous structs, type literals, utility types.
```rust
#[derive(Debug, Clone)]
pub struct Ty {
    pub kind: TyKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TyKind {
    Path { res: Res },
    Tuple { tys: Vec<Ty> },
    Array { ty: Box<Ty>, len: Const },
    Slice { ty: Box<Ty> },
    FnPtr { sig: FnSig },
    AnonStruct { fields: Vec<AnonField> },    // { x: i32, y: i32 }
    TypeLit { variants: Vec<Lit> },          // 200 | 404 | 500
    Utility { kind: UtilityKind, args: Vec<Ty> }, // Omit, Pick, ReturnType, Params
    Ref { mutability: Mutability, ty: Box<Ty> },  // &T or &mut T (no lifetime!)
    RawPtr { mutability: Mutability, ty: Box<Ty> }, // *mut T, *const T
    Infer,                                    // Type inference variable
    Err,
}

#[derive(Debug, Clone)]
pub struct AnonField {
    pub name: Symbol,
    pub ty: Ty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilityKind {
    Omit,
    Pick,
    ReturnType,
    Params,
    Partial,
    Required,
}
```

### Pat (src/hir_pat.rs)
```rust
#[derive(Debug, Clone)]
pub struct Pat {
    pub hir_id: HirId,
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatKind {
    Wild,                                          // _
    Binding { mode: BindingMode, name: Symbol, subpat: Option<Box<Pat>> },
    Struct { res: Res, fields: Vec<FieldPat>, rest: bool },
    Tuple { pats: Vec<Pat> },
    TupleStruct { res: Res, pats: Vec<Pat> },
    Path { res: Res },
    Lit { lit: Lit },
    Range { start: Option<Box<Pat>>, end: Option<Box<Pat>>, end_inclusive: bool },
    Or { pats: Vec<Pat> },
    Slice { prefix: Vec<Pat>, middle: Option<Box<Pat>>, suffix: Vec<Pat> },
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingMode {
    ByValue,
    ByRef { mutability: Mutability },
}
```

### Body (src/hir_body.rs)
```rust
#[derive(Debug, Clone)]
pub struct Body {
    pub params: Vec<Param>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub pat: Pat,
    pub ty: Ty,
    pub span: Span,
}
```

### LoweringContext (src/lowering.rs)
```rust
pub struct LoweringContext<'a> {
    pub interner: &'a Interner,
    pub resolved: &'a ResolvedCrate,
    pub crate_hir: Crate,
    // ID generators
    pub next_hir_id: u32,
    pub next_body_id: u32,
    // Current owner (for HirId generation)
    pub current_owner: DefId,
    // Local variable mapping: AST ident -> HirId
    pub local_map: FxHashMap<Symbol, HirId>,
    // Errors
    pub errors: Vec<LoweringError>,
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum LoweringError {
    #[error("cannot resolve `{name}` during lowering")]
    UnresolvedName { name: Symbol, span: Span },
    
    #[error("unsupported AST node: {kind}")]
    UnsupportedAst { kind: String, span: Span },
}
```

## Lowering Desugarings

### `for` loop -> `loop` + `match`
```rust
// AST: for x in iter { body }
// HIR: {
//   let mut _iter = IntoIterator::into_iter(iter);
//   loop {
//     match Iterator::next(&mut _iter) {
//       Some(x) => { body },
//       None => break,
//     }
//   }
// }
```

### `while` -> `loop` + `break`
```rust
// AST: while cond { body }
// HIR: loop { if cond { body } else { break } }
```

### `expr?` -> `match expr { Ok(v) => v, Err(e) => return Err(From::from(e)) }`

### `async fn` -> generator state machine (stub for now)
```rust
// AST: async fn foo() { body }
// HIR: fn foo() -> impl Future { generator { body } }
// For now: just mark as is_async = true, don't desugar fully
```

### `let chain` -> nested `if let`
```rust
// AST: if let Some(x) = a && let Some(y) = b && x > y { ... }
// HIR: if let Some(x) = a { if let Some(y) = b { if x > y { ... } } }
```

## Map (src/map.rs)
Like rustc's `hir::map`, provides lookup from HirId/DefId to HIR nodes.
```rust
pub struct Map<'hir> {
    pub crate_hir: &'hir Crate,
}

impl<'hir> Map<'hir> {
    pub fn item(&self, def_id: DefId) -> Option<&Item> { ... }
    pub fn body(&self, body_id: BodyId) -> Option<&Body> { ... }
    pub fn expr(&self, hir_id: HirId) -> Option<&Expr> { ... }
    pub fn ty(&self, hir_id: HirId) -> Option<&Ty> { ... }
    pub fn pat(&self, hir_id: HirId) -> Option<&Pat> { ... }
}
```

## Visitor (src/visitor.rs)
```rust
pub trait Visitor<'hir>: Sized {
    fn visit_crate(&mut self, crate_hir: &Crate) { ... }
    fn visit_item(&mut self, item: &Item) { ... }
    fn visit_expr(&mut self, expr: &Expr) { ... }
    fn visit_ty(&mut self, ty: &Ty) { ... }
    fn visit_pat(&mut self, pat: &Pat) { ... }
    fn visit_body(&mut self, body: &Body) { ... }
}

// Default walk_* implementations that recurse into children
pub fn walk_crate<'hir, V: Visitor<'hir>>(visitor: &mut V, crate_hir: &Crate) { ... }
pub fn walk_item<'hir, V: Visitor<'hir>>(visitor: &mut V, item: &Item) { ... }
// ... etc
```

## Public API (src/lib.rs)
```rust
pub use crate_hir::*;
pub use hir::*;
pub use hir_expr::*;
pub use hir_item::*;
pub use hir_ty::*;
pub use hir_pat::*;
pub use hir_body::*;
pub use hir_struct::*;
pub use ids::*;
pub use lowering::*;
pub use lowering_err::*;
pub use map::*;
pub use res::*;
pub use visitor::*;

pub mod tests;
```

## Tests

### Lowering Tests (tests/lowering.rs)
```rust
#[test]
fn lower_simple_fn() {
    // Parse `fn main() { let x = 1; }`
    // Lower to HIR
    // Assert HIR has one item (fn main), one body, one local
}

#[test]
fn lower_for_desugar() {
    // Parse `for x in 0..10 { }`
    // Lower to HIR
    // Assert HIR has `loop` + `match` structure
}
```

### Desugaring Tests (tests/desugaring.rs)
```rust
#[test]
fn desugar_while() { ... }
#[test]
fn desugar_try() { ... }
#[test]
fn desugar_let_chain() { ... }
```

For now, tests can be stubbed with `todo!()` — the important thing is the structure exists.

## CRITICAL: HRTB Support in HIR

HIR must be able to represent HRTB types even if we don't fully implement them yet:
```rust
// AST: for<T> fn(T) -> T
// HIR TyKind:
TyKind::FnPtr {
    sig: FnSig {
        inputs: vec![Ty { kind: TyKind::Bound { index: 0 }, span }],
        output: Ty { kind: TyKind::Bound { index: 0 }, span },
    },
    bound_vars: vec![BoundVarKind::Ty],
}
```

For now, store bound vars alongside the signature. Full HRTB support will come in the type checker.
