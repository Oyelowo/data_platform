//! End-to-end tests for out-of-process procedural macro expansion, driven by
//! manifest-based discovery (the same path a driver uses).

mod macro_fixture;

use macro_fixture::{parse_program, server_path, write_fixture_manifest};
use yelang_ast::ItemKind;
use yelang_macro::MacroExpander;
use yelang_macro::proc_macro::{
    ProcMacroClient, ProcMacroKind, ProcMacroRegistry, ProcMacroResolver, ProcMacroRuntime,
    ProcMacroSource,
};

fn runtime() -> ProcMacroRuntime {
    let manifest = write_fixture_manifest("test_macro");
    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    runtime
        .discover(&ProcMacroSource::Manifest(manifest))
        .expect("discover fixture manifest");
    runtime
}

#[test]
fn expand_fn_like_macro_through_server() {
    let runtime = runtime();
    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = make_answer!();
        }
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let fn_item = &result.program.items[0];
    let ItemKind::Fn(func) = &fn_item.kind else {
        panic!("expected fn")
    };
    assert_eq!(func.body.statements.len(), 1);
}

#[test]
fn expand_attribute_macro_through_server() {
    let runtime = runtime();
    let (program, interner) = parse_program(
        r#"
        @trace
        fn main() {}
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let fn_item = &result.program.items[0];
    let ItemKind::Fn(func) = &fn_item.kind else {
        panic!("expected fn")
    };
    assert_eq!(interner.resolve(&func.name.symbol), "main");
}

#[test]
fn expand_derive_macro_through_server() {
    let runtime = runtime();
    let (program, interner) = parse_program(
        r#"
        struct Foo;

        @derive(generate_const)
        struct Bar;
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    // Should have Foo, Bar, and the generated const.
    assert_eq!(result.program.items.len(), 3);
}

#[test]
fn expand_derive_macro_through_server_reports_invalid_item() {
    let runtime = runtime();
    let (program, interner) = parse_program(
        r#"
        struct Foo;

        @derive(answer)
        struct Bar;
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.to_string().contains("did not produce valid items")),
        "expected parse error for non-item derive output, got {:?}",
        result.errors
    );
}

#[test]
fn server_diagnostic_is_reported_as_expansion_error() {
    let runtime = runtime();
    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = emit_warning!(1);
        }
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    let diag = result
        .errors
        .iter()
        .find(|e| e.to_string().contains("intentional fixture warning"))
        .expect("expected warning diagnostic");
    assert!(
        matches!(
            diag,
            yelang_macro::ExpandError::ProcMacroDiagnostic {
                level: yelang_macro::DiagnosticLevel::Warning,
                ..
            }
        ),
        "expected warning severity, got {:?}",
        diag
    );
}

#[test]
fn server_panic_is_reported_as_expansion_error() {
    let runtime = runtime();
    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = explode!();
        }
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.to_string().contains("panicked")),
        "expected panic error, got {:?}",
        result.errors
    );
}

#[test]
fn missing_library_path_is_reported_as_expansion_error() {
    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut registry = ProcMacroRegistry::new();
    registry.register(
        "missing".to_string(),
        ProcMacroKind::FunctionLike,
        0,
        "/nonexistent/lib.dylib".to_string(),
        "missing_crate".to_string(),
        None,
    );
    let runtime = ProcMacroRuntime::new(client, ProcMacroResolver::new(registry));

    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = missing!();
        }
    "#,
    );
    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.to_string().contains("failed to load")),
        "expected load error, got {:?}",
        result.errors
    );
}

#[test]
fn runtime_caches_loaded_libraries() {
    let runtime = runtime();

    let first = runtime
        .resolve("make_answer", ProcMacroKind::FunctionLike)
        .unwrap()
        .unwrap();
    let second = runtime
        .resolve("trace", ProcMacroKind::Attribute)
        .unwrap()
        .unwrap();
    assert_eq!(
        first.library, second.library,
        "same library should be loaded once"
    );
    assert_eq!(runtime.loaded_library_count(), 1);
}

#[test]
fn resolution_is_kind_aware() {
    let runtime = runtime();
    // `make_answer` exists as a function-like macro; there is no derive with
    // that name.
    assert!(
        runtime
            .resolve("make_answer", ProcMacroKind::Derive)
            .is_none()
    );
    assert!(
        runtime
            .resolve("answer", ProcMacroKind::FunctionLike)
            .is_none()
    );
    assert!(runtime.resolve("answer", ProcMacroKind::Derive).is_some());
}

#[test]
fn unsafe_server_attribute_is_recognized() {
    let runtime = runtime();
    let (program, interner) = parse_program(
        r#"
        @unsafe(trace)
        fn main() {}
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
}
