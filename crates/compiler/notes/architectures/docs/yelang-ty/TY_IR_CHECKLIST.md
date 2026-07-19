# Type IR Production-Readiness Checklist

This checklist tracks the core type-system IR in `yelang-ty`.

## Data Model

- [x] `TyId` is a lifetime-free interned ID (`Id<TagTy>`) into `Interner::types`.
- [x] `Ty` is the recursive enum covering all Yelang type constructors.
- [x] `Ty::Projection` exists for associated type projections.
- [x] `Ty::Dynamic` holds a list of existential predicates.
- [x] `AliasTy` is used for opaque types / type aliases only.
- [x] `ProjectionTy` carries a `TraitRef` and associated item `DefId`.
- [x] `Predicate::NormalizesTo` exists.
- [x] `Predicate::WellFormed` exists.
- [x] `ConstId` is a lifetime-free interned ID (`Id<TagConst>`) into `Interner::consts`.
- [x] `Const` is the recursive enum of const constructors.
- [x] `Const::Param` exists for const generics.
- [x] `ParamConst` exists.

## Traversal

- [x] `TypeFolder` integrates with `Interner` for re-interning.
- [x] `TypeFoldable` implemented for `TyId`, `ConstId`, `GenericArg`, `List<T>`, `Binder<T>`.
- [x] `TypeSuperFoldable` covers every `Ty` constructor.
- [x] `TypeVisitor` supports custom inspection.
- [x] `TypeVisitable` implemented for `TyId` and `ConstId`.
- [x] `TypeSuperVisitable` covers every `Ty` constructor.

## Substitution and Shifting

- [x] `substitute` applies `Substitution` to type-like values.
- [x] `shift_out_to_binder` shifts De Bruijn indices out.
- [x] `shift_in` shifts De Bruijn indices in.
- [x] `instantiate_binder` replaces bound vars with concrete args.

## Interner

- [x] `mk_ty` deduplicates types in `IndexVec<TyId, Ty>`.
- [x] `mk_const` deduplicates constants in `IndexVec<ConstId, ConstData>`.
- [x] `mk_generic_args` deduplicates generic-arg lists.
- [x] `mk_bound_var_list` deduplicates bound-var lists.
- [x] `mk_existential_predicates` deduplicates existential-predicate lists.
- [x] `mk_anon_struct_fields` deduplicates anon-struct field lists.

## Tests

- [x] Unit tests for interning (types and constants).
- [x] Unit tests for folding.
- [x] Unit tests for visiting.
- [x] Unit tests for substitution.
- [x] Unit tests for binder shifting and instantiation.
