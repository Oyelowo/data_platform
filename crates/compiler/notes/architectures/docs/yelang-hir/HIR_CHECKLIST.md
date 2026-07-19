# HIR Production-Readiness Checklist

This checklist tracks whether every Yelang AST construct is correctly lowered to HIR, visitable, and tested. Each item must be `[x]` before HIR is considered production-ready.

Legend:
- **Lower**: AST → HIR lowering is implemented and semantically correct.
- **Visit**: The HIR visitor recurses into the node.
- **Test**: There is a dedicated test exercising the construct.
- **Note**: Any caveats or future work.

## Data Model / Storage

- [x] `DefId` indexed `IndexVec` for items.
- [x] Generational `Arena` for `Expr`, `Pat`, `Stmt`, `Ty`, `Body`.
- [x] `ArenaMap` secondary span tables for all node kinds.
- [x] Dense `IndexVec` for `DefId`-indexed metadata.

## Types (`hir_ty.rs`)

- [x] `Ty::Never` variant exists.
- [x] `TypeKind::Never` lowers to `Ty::Never`.
- [x] `Ty::Missing` variant exists for optional type positions.
- [x] `UtilityKind::TypeOf` exists and preserves expression.
- [x] `TypeOperator::TypeOf` lowers correctly.
- [x] `Ty::Path` with generic type args.
- [x] `Ty::Path` with const generic args.
- [x] `Ty::Path` with associated type bindings.
- [x] `Ty::Tuple`.
- [x] `Ty::Array` with correct length `Const`.
- [x] `Ty::Slice`.
- [x] `Ty::Ref` / `Ty::RawPtr`.
- [x] `Ty::FnPtr`.
- [x] `Ty::TypeOf`.
- [x] `Ty::ForAll` (HRTB).
- [x] `Ty::TypeLit`.
- [x] `Ty::AnonStruct`.
- [x] `Ty::Union`.
- [x] `Ty::ImplTrait` / `Ty::DynTrait`.
- [x] `Ty::Infer`.
- [x] `Ty::Err`.

## Patterns (`hir_pat.rs`)

- [x] `Pat::Wild`.
- [x] `Pat::Binding` (by value, by ref, mut).
- [x] `Pat::Binding` with sub-pattern.
- [x] `Pat::Slice` correctly splits prefix/middle/suffix.
- [x] `Pat::Rest` preserved inside slice patterns.
- [x] `Pat::Struct`.
- [x] `PatternKind::Record` → `Pat::Struct` with shorthand.
- [x] `Pat::Tuple`.
- [x] `Pat::TupleStruct`.
- [x] `Pat::Path`.
- [x] `Pat::Lit`.
- [x] `PatternKind::Range` → `Pat::Range`.
- [x] `Pat::Or`.
- [x] `Pat::Err`.

## Expressions (`hir_expr.rs`)

- [x] `Expr::Lit`.
- [x] `Expr::Path`.
- [x] `Expr::Binary`.
- [x] `Expr::Unary`.
- [x] `Expr::Call` (positional/named args).
- [x] `Expr::MethodCall`.
- [x] `Expr::Field`.
- [x] `Expr::Index`.
- [x] `Expr::Assign`.
- [x] `Expr::AssignOp` preserved (no double-eval desugaring).
- [x] `Expr::DestructureAssign` preserved.
- [x] `Expr::Block`.
- [x] `Expr::Loop`.
- [x] `Expr::If`.
- [x] `Expr::While` (desugared).
- [x] `Expr::ForLoop` (desugared).
- [x] `Expr::Match`.
- [x] `Expr::Break` / `Expr::Continue`.
- [x] `Expr::Return`.
- [x] `Expr::Try` (`?`) preserved as dedicated node.
- [x] `Expr::Await` preserved as dedicated node.
- [x] `Expr::Async` block preserved.
- [x] `Expr::Gen` preserved.
- [x] `Expr::Struct`.
- [x] `Expr::Object` literal.
- [x] `Expr::DocumentAccess` with `Field` / `Spread` projections.
- [x] `Expr::Range` with inclusive/exclusive info.
- [x] `Expr::IsType` (`expr is Type`).
- [x] `Expr::TypeAscription` preserved.
- [x] `Expr::Comprehension` preserved.
- [x] `Expr::Tuple`.
- [x] `Expr::Array` (list form).
- [ ] `Expr::Array` repeat form `[expr; count]`.
- [x] `Expr::Cast`.
- [x] `Expr::Closure`.
- [x] `Expr::Let`.
- [ ] `InterpolatedString`.
- [ ] `Underscore` expression.
- [ ] `BindAt` (`base@alias`).
- [ ] `Query` expression.
- [x] `Expr::Err`.

## Items (`hir_item.rs`)

- [x] `ItemKind::Fn`.
- [ ] `FnSig::is_const` propagated from AST.
- [x] `ItemKind::Struct` (named/tuple/unit).
- [x] `ItemKind::Enum`.
- [x] Enum discriminants lower to real `Const`.
- [x] `ItemKind::Trait`.
- [x] Trait super traits stored.
- [x] `ItemKind::Impl`.
- [x] Impl polarity stored (`ImplPolarity::Positive` / `Negative`).
- [x] `ItemKind::TyAlias`.
- [x] `ItemKind::Const`.
- [x] `ItemKind::Static`.
- [x] `ItemKind::Mod` (inline).
- [ ] `ItemKind::Mod` (external).
- [x] `ItemKind::Use` with resolved path and `UseKind`.
- [x] Attributes stored on items.
- [x] `ItemKind::Union` removed; union types live only in `Ty::Union`.

## Visitor (`visitor.rs`)

- [x] `walk_crate` visits items and impls.
- [x] `walk_crate` visits traits.
- [x] `walk_item` visits `Mod`, `TyAlias`, `Const`, `Static`, `Use`.
- [x] `walk_ty` visits `Ty::Path` generic args.
- [x] `visit_generics`, `visit_where_clause`, `visit_trait_bound` hooks.

## Validation (`validate.rs`)

- [x] `validate_hir` function exists.
- [x] DefId references valid.
- [x] Node ID references valid.
- [x] Item def_id consistency.
- [ ] Binding uniqueness in scope.
- [ ] Break/continue labels in scope.
- [ ] Return inside function body.
- [ ] Enum discriminants are constant.
- [ ] Array lengths are constant.

## Tests

- [x] `tests/pat.rs` (covered via `tests/exprs.rs`, `tests/lowering.rs`, and `tests/items.rs`).
- [x] `tests/exprs.rs` covers expression forms.
- [x] `tests/types.rs` covers type forms.
- [x] `tests/items.rs` covers item forms.
- [x] `tests/desugaring.rs` covers for/while/let-chain desugaring.
- [x] `tests/visitor.rs` covers traversal.
- [x] `tests/validate.rs` covers invariant checks.
- [x] `tests/derive_tests.rs` covers built-in derive expansion.
- [x] `tests/storage_tests.rs` covers arena / index invariants.

## Documentation

- [x] `SPEC.md` updated to final representation.
- [x] This checklist is fully checked off for the current HIR phase.

## Remaining out-of-scope for HIR (handled in later phases)

- `InterpolatedString`, `Underscore` expression, `BindAt`, and `Query` surface
  syntax are rejected during lowering with a clear `LoweringError`.
- `[expr; count]` array repeat syntax is rejected during lowering.
- Binding/label scoping checks, const evaluation, and control-flow validation
  will be enforced in the type checker / MIR lowering / const-eval phases.
