# Phase F Design: Clarifying the HIR / Semantic Type Boundary

## The question

`HirTy` and `Ty` both look like "the type representation". Should they be the
same thing?

## Answer

They are the same *name* at different layers. Following the convention used by
**rustc** and **rust-analyzer**, the HIR-level type node is named `Ty` and is
disambiguated by module path:

| Layer | rustc | rust-analyzer | Yelang |
|-------|-------|---------------|--------|
| AST / source syntax | `ast::Ty` | `syntax::ast::Type` | `yelang_ast::Type` |
| HIR (lowered, resolved paths) | `hir::Ty` | `hir::Type` | `yelang_hir::hir::Ty` |
| Semantic / interned | `rustc_middle::ty::Ty` | `hir_ty::Ty` | `yelang_ty::Ty` |

Because Yelang stores HIR types in an arena, the arena key keeps a distinct name:
`HirTyId`. This is the id of an HIR type node, not the semantic type id
(`yelang_ty::TyId`).

## Why HIR cannot literally use `yelang_ty::Ty`

1. **HIR is built before `TyCtxt` exists.** `TyId` values are interned in the
   `TyCtxt` interner. HIR construction happens during lowering from the AST,
   before any semantic type context is available.
2. **Source-specific forms.** HIR types must represent constructs that are not
   part of the semantic type system, such as:
   - `typeof expr`
   - type literals (`"pending" | "active"`)
   - utility types (`Omit<T, K>`, `Pick<T, K>`)
   - `impl Trait` / `dyn Trait` source forms
   - `for<T>` binders in source syntax
   These are lowered into simpler semantic types (or rejected) during type
   checking, not during HIR construction.
3. **Span and error recovery.** `yelang_hir::hir::Ty` carries `Span` information
   and distinct `Infer` / `Missing` / `Err` variants for error reporting.
   `yelang_ty::Ty` is interned and span-free.

## What we did in Phase F

1. Renamed `HirTy` → `Ty` in `yelang-hir/src/hir/ty.rs`.
2. Kept `HirTyId` in `yelang-hir/src/ids.rs` as the HIR type-node id.
3. Updated every use site, import, and doc comment across the workspace.
4. In `yelang-tycheck`, where both HIR and semantic `Ty` are needed, we import
   `yelang_hir as hir` and write `hir::Ty` for the HIR type node, leaving
   unqualified `Ty` for the semantic type.
5. Kept the lowering pipeline intact:
   - `yelang-tycheck/src/hir_ty_lower.rs` lowers `hir::Ty` to `yelang_ty::Ty`.
   - `yelang-tycheck/src/collector.rs` builds semantic signatures from the
     lowered types.
6. Added tests for source-specific lowering edge cases (`typeof`, `impl Trait`,
   `_`, missing).

## Phase F file tree

```text
yelang-hir/
├── src/
│   ├── ids.rs               # HirTyId defined here
│   ├── hir/
│   │   ├── ty.rs            # Ty (HIR type node) defined here
│   │   ├── core.rs          # uses HirTyId for FnSig, fields, etc.
│   │   ├── adt.rs           # field types use HirTyId
│   │   ├── item.rs          # item signatures use HirTyId
│   │   ├── expr.rs          # casts/ascryptions use HirTyId
│   │   └── body.rs          # param types use HirTyId
│   ├── crate_data.rs        # type arena stores Ty
│   ├── map.rs               # lookup maps HirTyId -> Ty
│   ├── lowering/            # builds Ty from AST
│   ├── visit/               # visitor/folder/mut_visitor over Ty
│   ├── derive/              # derive macros read Ty
│   ├── validate.rs          # validates Ty
│   └── tests/               # HIR tests updated
yelang-tycheck/
├── src/
│   ├── hir_ty_lower.rs      # lowers hir::Ty -> yelang_ty::Ty
│   ├── collector.rs         # consumes lowered hir::Ty
│   ├── check.rs             # body checking uses yelang_ty::Ty
│   └── tests/mod.rs         # tests updated
yelang-ty/
└── src/ty.rs                # Ty / TyId unchanged
```

## Checklist

- [x] Decide that HIR types stay syntactic and semantic types stay interned.
- [x] Rename `HirTy` → `Ty` in `yelang-hir`.
- [x] Keep `HirTyId` as the HIR type-node id.
- [x] Update all imports/usages in `yelang-hir`.
- [x] Update all imports/usages in `yelang-tycheck` (using `hir::Ty` where
      disambiguation is needed).
- [x] Update doc comments and error messages.
- [x] Add tests for `hir::Ty` → `yelang_ty::Ty` lowering edge cases.
- [x] `cargo test --workspace` passes.

## Tests added

- `syntax_ty_infer_in_body_is_fresh_var`
- `syntax_ty_missing_in_body_is_fresh_var`
- `syntax_ty_typeof_in_body_lowers_to_expr_type`
- `syntax_ty_impl_trait_lowers_to_alias`

These verify that source-level type forms (`_`, missing, `typeof`, `impl Trait`)
survive into HIR and are lowered correctly by `hir_ty_lower`.

## Future work (not Phase F)

- Moving more purely-semantic variants out of `hir::Ty` once the language
  settles (e.g. if `Union` becomes a real semantic type, it can move to
  `yelang_ty::Ty`).
- Source-span side tables for `yelang_ty::Ty` if diagnostics need them.

## Why not `HirTy`?

`HirTy` works and avoids collisions, but it gives the HIR type a special
synthetic name. `Ty` is the natural name for a type; the layer is already
carried by the module path (`hir::Ty` vs `ty::Ty`). This matches how rustc and
rust-analyzer name their layers and keeps the most common semantic type as the
unqualified `Ty` in type-system code.

The one place where a prefix is unavoidable is the arena key: because
`yelang_ty::TyId` already exists, the HIR type-node id is `HirTyId`. That is
honest — it is the id of an HIR node, not the interned semantic type id.
