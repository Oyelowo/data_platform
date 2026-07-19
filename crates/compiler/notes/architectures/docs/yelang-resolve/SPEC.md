# Specification: yelang-resolve

## Overview
Name resolution crate for Yelang. Two-phase resolution (early + late) with rib-based scoping, three namespaces, and import resolution.

## Dependencies
- `yelang-ast` (path: `../yelang-ast`)
- `yelang-lexer` (path: `../yelang-lexer`) — for Span, FileId
- `yelang-interner` (path: `../yelang-interner`) — for Symbol
- `yelang-util` (path: `../yelang-util`) — for FxHashMap, OrderedMap, DefId
- `thiserror` (workspace)

## CRITICAL RULES
1. **Named struct variants ONLY**: Every enum variant with >1 field MUST be a named struct variant. NEVER use tuple variants for >1 field.
2. **mod.rs style**: Every module directory has a `mod.rs` file.
3. **Max file size**: ~500 lines. Split by concern.
4. **No unwrap/expect in library code**: Use `Result` or `Option`. Only `unwrap` in tests.
5. **Error types**: Use `thiserror` for all error enums.
6. **Use yelang-util wrappers**: `FxHashMap`, `FxHashSet`, `OrderedMap` from `yelang-util`.
7. **Use yelang-util Id types**: `DefId` from `yelang_util::id`.

## Module Structure

```
src/
  lib.rs              — Public exports
  error.rs            — ResolutionError enum with thiserror
  namespaces.rs       — Namespace enum (Value, Type, Macro)
  rib.rs              — Rib, RibKind, binding stacks
  module_tree.rs      — ModuleNode, ModuleTree (hierarchical module structure)
  scope.rs            — Scope, lookup operations
  def_collector.rs    — Phase 0: collect all DefIds before resolution
  early.rs            — Early resolution: build module tree, resolve imports, collect macros
  late.rs             — Late resolution: resolve all paths, expressions, types, patterns
  imports.rs          — Import resolution (fixed-point algorithm for use statements)
  path.rs             — Path resolution logic
  tests/
    mod.rs
    basic.rs          — Basic resolution tests
    namespaces.rs     — Namespace separation tests
    shadowing.rs      — Name shadowing tests
    imports.rs        — Import resolution tests
    generics.rs       — Generic parameter resolution tests
    errors.rs         — Error case tests
```

## Core Data Structures

### Namespace (src/namespaces.rs)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    Value,  // variables, functions, constants, statics
    Type,   // structs, enums, traits, type aliases, modules
    Macro,  // macros, decorators
}
```

### Rib (src/rib.rs)
```rust
#[derive(Debug, Clone)]
pub struct Rib {
    pub kind: RibKind,
    // Separate bindings per namespace
    pub bindings: FxHashMap<Namespace, FxHashMap<Symbol, Resolution>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibKind {
    Module,      // Module scope: items visible throughout module
    Fn,          // Function scope: params + locals visible, nested fns opaque
    Block,       // Block scope: let bindings visible in this + nested blocks
    Loop,        // Loop scope: loop labels
    Pat,         // Pattern scope: bindings from match arm, if let, etc.
    Opaque,      // Opaque scope: nothing from outside visible
    Macro,       // Macro expansion scope: hygiene boundary
}
```

### Resolution (src/rib.rs)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Def { def_id: DefId },
    Local { local_id: u32 },  // Local variable (no DefId, just a local index)
    Import { import_id: DefId },
    Err,
}
```

### ModuleTree (src/module_tree.rs)
```rust
#[derive(Debug, Clone)]
pub struct ModuleTree {
    pub root: ModuleNode,
    pub modules: FxHashMap<DefId, ModuleNode>,
}

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub def_id: DefId,
    pub name: Symbol,
    pub parent: Option<DefId>,
    pub children: Vec<DefId>,
    pub items: FxHashMap<Namespace, FxHashMap<Symbol, DefId>>,
}
```

### Resolver (src/scope.rs or src/resolver.rs)
```rust
pub struct Resolver<'a> {
    pub interner: &'a Interner,
    pub module_tree: ModuleTree,
    pub next_local_id: u32,
    // Rib stacks per namespace
    pub value_ribs: Vec<Rib>,
    pub type_ribs: Vec<Rib>,
    pub macro_ribs: Vec<Rib>,
    // Import resolution
    pub unresolved_imports: Vec<UnresolvedImport>,
    // Errors
    pub errors: Vec<ResolutionError>,
    // Already collected DefIds
    pub definitions: FxHashMap<DefId, Definition>,
}
```

### ResolutionError (src/error.rs)
Use `thiserror`:
```rust
#[derive(thiserror::Error, Debug, Clone)]
pub enum ResolutionError {
    #[error("cannot find `{name}` in this scope")]
    NotFound { name: Symbol, span: Span },
    
    #[error("`{name}` is ambiguous")]
    Ambiguous { name: Symbol, span: Span, candidates: Vec<DefId> },
    
    #[error("`{name}` is a {found}, expected a {expected}")]
    WrongNamespace { name: Symbol, found: Namespace, expected: Namespace, span: Span },
    
    #[error("circular import")]
    CircularImport { span: Span },
    
    #[error("`{name}` defined multiple times")]
    DuplicateDefinition { name: Symbol, span: Span, original_span: Span },
}
```

## Implementation Order

1. `error.rs` — Define all error types
2. `namespaces.rs` — Simple enum
3. `rib.rs` — Rib, RibKind, Resolution
4. `module_tree.rs` — ModuleNode, ModuleTree
5. `def_collector.rs` — Walk AST, collect all items, assign DefIds
6. `scope.rs` — Resolver struct, push/pop ribs, resolve_name
7. `early.rs` — Early resolution: build module tree, resolve imports, collect macros
8. `imports.rs` — Fixed-point import resolution
9. `late.rs` — Late resolution: resolve paths in exprs, types, patterns
10. `path.rs` — Path resolution (segment by segment)
11. `lib.rs` — Public API: `resolve_crate(ast: &Crate) -> ResolvedCrate`

## Public API (src/lib.rs)

```rust
pub use error::*;
pub use namespaces::*;
pub use rib::*;
pub use module_tree::*;
pub use scope::*;

pub mod tests;

/// Result of resolving a crate.
#[derive(Debug, Clone)]
pub struct ResolvedCrate {
    pub module_tree: ModuleTree,
    pub definitions: FxHashMap<DefId, Definition>,
    pub errors: Vec<ResolutionError>,
}

/// The main entry point for name resolution.
pub fn resolve_crate(ast: &yelang_ast::Program, interner: &Interner) -> ResolvedCrate {
    let mut resolver = Resolver::new(interner);
    resolver.resolve(ast);
    ResolvedCrate {
        module_tree: resolver.module_tree,
        definitions: resolver.definitions,
        errors: resolver.errors,
    }
}
```

## Tests

Each test file should have at least 5 test cases. Use `rstest` for parameterized tests.

### Example test (tests/basic.rs)
```rust
use yelang_resolve::*;

#[test]
fn resolve_fn_call() {
    // Given a parsed AST with a function call
    // When resolved
    // Then the function name resolves to a DefId
}
```

For now, tests can be stubbed with `todo!()` — the important thing is the test structure exists.

## CRITICAL: No lifetimes
Since Yelang has no lifetimes, the resolver does NOT need to handle lifetime resolution. This is a major simplification over rustc.
