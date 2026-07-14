# Yelang Compiler Design Document
## Phase: Name Resolution & HIR

---

## 1. Architecture Decisions

### 1.1 Compile-Time vs Runtime

| Mechanism | Phase | Example |
|---|---|---|
| Built-in decorators | Early expansion | `@repr(C)`, `@no_std` |
| User-defined macros | AST→AST transformation | `@derive(Show)`, `@table` |
| CTFE (`comptime`) | Type-check time | `comptime compute_schema()` |

Decorators/macros are **erased before HIR**. No runtime reflection.

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

Three namespaces, no lifetime namespace:
- `ValueNS` -- variables, functions, constants
- `TypeNS` -- types, traits, modules
- `MacroNS` -- macros, decorators

### 1.5 Resolution Pipeline

```
AST
  ↓
Early Resolution (modules, imports, macro names)
  ↓
Macro Expansion (iterative)
  ↓
Late Resolution (all paths in exprs, types, patterns)
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
yelang-util          (wrapper types: FxHashMap, SlotMap, etc.)
  ↑
yelang-interner      (string interning)
  ↑
yelang-lexer         (tokenization)
  ↑
yelang-ast           (AST types + parser)
  ↑
yelang-macro         (macro expansion + hygiene)
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
| Macros/decorators | 6 | `@derive`, `@repr` |
| Error cases | 12 | Undefined, ambiguous, cycle |

### 3.2 HIR Lowering Tests (yelang-hir)

| Category | Count | Example |
|---|---|---|
| Expression lowering | 12 | Binary, call, method call, field |
| Item lowering | 10 | Fn, struct, enum, trait, impl |
| Type lowering | 8 | Path, tuple, fn ptr, anon struct |
| Pattern lowering | 6 | Binding, struct, tuple |
| Desugaring | 8 | For, while, try, async |
| Decorator expansion | 4 | `@derive` generates impls |

---

## 4. Code Quality Standards

1. **Named struct variants**: Every enum variant with >1 field must be named
2. **Wrapper types**: All external collections wrapped in yelang-util
3. **Error types**: Every fallible operation returns `Result<T, E>` with typed errors
4. **Tests**: Every module has tests; integration tests in `tests/` directory
5. **Documentation**: Every public type/function has doc comments
6. **No unwrap/expect in library code**: Only in tests
