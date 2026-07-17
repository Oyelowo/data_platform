# Yelang Compiler Design Document
## Phase: Name Resolution & HIR

> **Note (2026-07-17):** User-defined macros have been removed from the active design. See `notes/architectures/adr_no_user_defined_macros.md`. Built-in decorators/derives are handled directly by the compiler; user extensibility will be provided by `comptime` after the backend is ready.

---

## 1. Architecture Decisions

### 1.1 Compile-Time vs Runtime

| Mechanism | Phase | Example |
|---|---|---|
| Built-in decorators | Early expansion / HIR lowering | `@repr(C)`, `@no_std`, `@derive(Clone)` |
| CTFE (`comptime`) | Type-check / codegen time | `comptime compute_schema()` |

Built-in decorators are **erased before HIR**. No runtime reflection.

### 1.2 Module System

- `mod.rs` style for all module directories
- Named struct variants for all enum variants with >1 field
- Max file size: ~500 lines; split when exceeded
- Wrapper types around all external dependencies

### 1.3 Identifier System (Simplified rustc)

```
DefId = (CrateId, DefIndex)     -- cross-crate definition
LocalDefId = u32                -- current crate only
HirId = (LocalDefId, u32)       -- any HIR node
BodyId = HirId                  -- executable code
```

### 1.4 Namespaces

Two namespaces, no lifetime namespace and no macro namespace:
- `ValueNS` -- variables, functions, constants
- `TypeNS` -- types, traits, modules

### 1.5 Resolution Pipeline

```
AST
  ↓
Early Resolution (modules, imports)
  ↓
Late Resolution (all paths in exprs, types, patterns)
  ↓
Built-in attribute/derive lowering (direct HIR generation)
  ↓
AST → HIR Lowering (desugar + resolve names to DefIds)
  ↓
HIR Crate
```

### 1.6 HIR Design Principles

- All names resolved to `DefId` or `HirId`
- All syntax sugar desugared (`for` → `loop`, `?` → `match`, `async` → generator)
- Out-of-band storage: maps keyed by `DefId` / `BodyId`
- No lifetimes in HIR types (we don't have lifetimes!)
- `Binder<T>` for HRTB at the type level, but no De Bruijn indices in HIR

### 1.7 Type System Support (Future Hooks)

HIR must accommodate:
- Anonymous structs: `{ x: i32, y: i32 }`
- Type literals: `200 | 404 | 500`
- Utility types: `Omit<User, "password">`, `Pick<User, "name">`, `ReturnType<typeof(foo)>`
- HRTB: `for<T> fn(T) -> T`

### 1.8 File Size Limits

- Target: <500 lines per source file
- Split by: data type (Expr/Item/Ty/Pat), by operation (parse/lower/visit)
- Each module directory has `mod.rs`

---

## 2. Crate Graph

```
yelang-interner      (string interning)
  ↑
yelang-lexer         (tokenization)
  ↑
yelang-ast           (AST types + parser)
  ↑
yelang-resolve       (name resolution: DefId, ribs, scopes)
  ↑
yelang-hir           (HIR types + lowering)
```

---

## 3. TDD Test Matrix

### 3.1 Name Resolution Tests (yelang-resolve)

| Category | Count | Example |
|---|---|---|
| Basic value resolution | 10 | `fn main() { foo(); }` |
| Type namespace | 8 | `type x = i32; let x: x = 1;` |
| Shadowing | 6 | Nested `let` bindings |
| Imports | 10 | `use`, `use as`, `use *` |
| Generics | 8 | `fn id<T>(x: T) -> T` |
| Traits | 8 | `impl`, trait method call |
| Built-in decorators/derives | 6 | `@derive(Clone)`, `@repr` |
| Error cases | 12 | Undefined, ambiguous, cycle |

### 3.2 HIR Lowering Tests (yelang-hir)

| Category | Count | Example |
|---|---|---|
| Expression lowering | 12 | Binary, call, method call, field |
| Item lowering | 10 | Fn, struct, enum, trait, impl |
| Type lowering | 8 | Path, tuple, fn ptr, anon struct |
| Pattern lowering | 6 | Binding, struct, tuple |
| Desugaring | 8 | For, while, try, async |
| Built-in derive lowering | 4 | `@derive(Clone)` generates impls |

---

## 4. Code Quality Standards

1. **Named struct variants**: Every enum variant with >1 field must be named
2. **Wrapper types**: All external collections wrapped in yelang-util
3. **Error types**: Every fallible operation returns `Result<T, E>` with typed errors
4. **Tests**: Every module has tests; integration tests in `tests/` directory
5. **Documentation**: Every public type/function has doc comments
6. **No unwrap/expect in library code**: Only in tests
