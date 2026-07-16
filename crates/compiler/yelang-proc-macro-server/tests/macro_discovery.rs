//! Integration tests for Phase 7.5 cross-crate proc-macro discovery:
//! manifest-based discovery, server introspection, freshness validation,
//! duplicate detection, and the driver-facing API.

mod macro_fixture;

use std::path::{Path, PathBuf};

use macro_fixture::{
    FIXTURE_MACROS, fixture_dylib_path, fixture_manifest, parse_program, server_path,
    write_fixture_manifest,
};
use yelang_ast::ItemKind;
use yelang_macro::MacroExpander;
use yelang_macro::proc_macro::{
    DiscoveryError, ProcMacroClient, ProcMacroKind, ProcMacroRegistry, ProcMacroResolver,
    ProcMacroRuntime, ProcMacroSource, Provenance,
};

fn runtime_with(
    source: ProcMacroSource,
) -> (ProcMacroRuntime, yelang_macro::proc_macro::DiscoveredCrate) {
    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let discovered = runtime.discover(&source).expect("discovery should succeed");
    (runtime, discovered)
}

fn registry_count(runtime: &ProcMacroRuntime) -> usize {
    runtime.resolver().registry().iter().count()
}

// --- Manifest discovery -------------------------------------------------

#[test]
fn manifest_discovery_registers_every_macro_without_loading() {
    let manifest = write_fixture_manifest("test_macro");
    let (runtime, discovered) = runtime_with(ProcMacroSource::Manifest(manifest));

    assert_eq!(discovered.crate_name, "test_macro");
    assert_eq!(discovered.provenance, Provenance::Manifest);
    assert_eq!(discovered.macro_ids.len(), FIXTURE_MACROS.len());

    // Discovery != loading: nothing has been dlopened yet.
    assert_eq!(runtime.loaded_library_count(), 0);

    // Every macro is registered with crate attribution, kind, and index.
    let registry = runtime.resolver().registry();
    for (index, (name, kind)) in FIXTURE_MACROS.iter().enumerate() {
        let def = registry.find(name, *kind).unwrap_or_else(|| {
            panic!("macro {name} ({kind:?}) not registered");
        });
        assert_eq!(def.crate_name, "test_macro");
        assert_eq!(def.macro_index, index as u32);
    }
}

#[test]
fn manifest_discovery_expands_all_three_kinds_end_to_end() {
    let manifest = write_fixture_manifest("test_macro");
    let (runtime, _) = runtime_with(ProcMacroSource::Manifest(manifest));
    let (program, interner) = parse_program(
        r#"
        @trace
        fn main() {
            let x = make_answer!();
        }

        @derive(generate_const)
        struct Bar;
    "#,
    );

    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    // fn main, struct Bar, generated const.
    assert_eq!(result.program.items.len(), 3);
    let ItemKind::Fn(func) = &result.program.items[0].kind else {
        panic!("expected fn")
    };
    assert_eq!(func.body.statements.len(), 1);
}

#[test]
fn noncanonical_dylib_path_is_canonicalized() {
    let dylib = fixture_dylib_path();
    // Spell the dylib path with a `..` segment; all components exist.
    let noncanonical = dylib
        .parent()
        .unwrap()
        .join("..")
        .join(dylib.parent().unwrap().file_name().unwrap())
        .join(dylib.file_name().unwrap());
    let mut manifest = fixture_manifest(&dylib, "test_macro");
    manifest.dylib.path = noncanonical;

    let dir = std::env::temp_dir().join(format!("yelang-disc-noncanon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_path = dir.join("test_macro.ypm.json");
    manifest.write(&manifest_path).unwrap();

    let (runtime, _) = runtime_with(ProcMacroSource::Manifest(manifest_path));
    let def = runtime
        .resolver()
        .resolve("make_answer", ProcMacroKind::FunctionLike)
        .unwrap();
    assert!(!def.library_path.contains(".."));
    assert!(Path::new(&def.library_path).is_absolute());

    // Both macros resolve to one and the same cached library handle.
    let first = runtime
        .resolve("make_answer", ProcMacroKind::FunctionLike)
        .unwrap()
        .unwrap();
    let second = runtime
        .resolve("trace", ProcMacroKind::Attribute)
        .unwrap()
        .unwrap();
    assert_eq!(first.library, second.library);
    assert_eq!(runtime.loaded_library_count(), 1);
}

// --- Introspection fallback ----------------------------------------------

#[test]
fn introspection_discovers_exports_and_preseeds_the_cache() {
    let dylib = fixture_dylib_path();
    let (runtime, discovered) = runtime_with(ProcMacroSource::Dylib(dylib.clone()));

    assert_eq!(discovered.crate_name, "test_macro");
    assert_eq!(
        discovered.provenance,
        Provenance::Introspected,
        "no sidecar manifest should exist next to the fixture dylib"
    );
    assert_eq!(discovered.macro_ids.len(), FIXTURE_MACROS.len());

    // Introspection loaded the library once and pre-seeded the cache...
    assert_eq!(runtime.loaded_library_count(), 1);

    // ...so resolving a macro does not load it again.
    let resolved = runtime
        .resolve("make_answer", ProcMacroKind::FunctionLike)
        .unwrap()
        .unwrap();
    assert_eq!(runtime.loaded_library_count(), 1);
    assert_eq!(resolved.macro_index, 0);

    // End-to-end expansion through the introspected registration.
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
}

#[test]
fn dylib_source_probes_sidecar_manifest_first() {
    let dylib = fixture_dylib_path();

    // Isolated copy of the dylib with a sidecar manifest next to it, so the
    // probe is exercised without racing other tests over target/debug.
    let dir = std::env::temp_dir().join(format!("yelang-disc-sidecar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dylib_copy = dir.join(format!(
        "{}probe_macro{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    std::fs::copy(&dylib, &dylib_copy).unwrap();

    let mut manifest = fixture_manifest(&dylib_copy, "sidecar_probe");
    manifest.dylib.path = PathBuf::from(dylib_copy.file_name().unwrap());
    let sidecar = yelang_macro::proc_macro::sidecar_manifest_path(&dylib_copy);
    manifest.write(&sidecar).unwrap();

    let (runtime, discovered) = runtime_with(ProcMacroSource::Dylib(dylib_copy));
    assert_eq!(
        discovered.provenance,
        Provenance::Manifest,
        "sidecar manifest should win over introspection"
    );
    assert_eq!(discovered.crate_name, "sidecar_probe");
    assert_eq!(runtime.loaded_library_count(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

// --- Freshness validation -------------------------------------------------

fn discover_with_doctored_manifest(
    tag: &str,
    doctor: impl FnOnce(&mut yelang_macro::proc_macro::ProcMacroCrateManifest),
) -> DiscoveryError {
    let dylib = fixture_dylib_path();
    let mut manifest = fixture_manifest(&dylib, "test_macro");
    // Absolute dylib path: the doctored manifest lives in a temp dir. Set
    // before doctoring so a doctor can deliberately re-point it.
    manifest.dylib.path = dylib.clone();
    doctor(&mut manifest);

    let dir =
        std::env::temp_dir().join(format!("yelang-disc-doctored-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_path = dir.join("doctored.ypm.json");
    manifest.write(&manifest_path).unwrap();

    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let error = runtime
        .discover(&ProcMacroSource::Manifest(manifest_path))
        .expect_err("doctored manifest must be rejected");
    let _ = std::fs::remove_dir_all(&dir);
    error
}

#[test]
fn tampered_hash_is_rejected() {
    let error = discover_with_doctored_manifest("hash", |m| {
        m.dylib.content_hash = "blake3:deadbeef".to_string();
    });
    assert!(
        matches!(error, DiscoveryError::HashMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn tampered_size_is_rejected() {
    let error = discover_with_doctored_manifest("size", |m| {
        m.dylib.size += 1;
    });
    assert!(
        matches!(error, DiscoveryError::SizeMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn wrong_protocol_version_is_rejected_before_loading() {
    let error = discover_with_doctored_manifest("protocol", |m| {
        m.protocol_version += 1;
    });
    assert!(
        matches!(error, DiscoveryError::ProtocolMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn wrong_host_triple_is_rejected_before_loading() {
    let error = discover_with_doctored_manifest("triple", |m| {
        m.host_triple = "definitely-not-the-host-triple".to_string();
    });
    assert!(
        matches!(error, DiscoveryError::TripleMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn missing_dylib_is_reported_at_discovery() {
    let error = discover_with_doctored_manifest("missing", |m| {
        m.dylib.path = PathBuf::from("/nonexistent/libgone.dylib");
    });
    assert!(matches!(error, DiscoveryError::Io { .. }), "got {error:?}");
}

#[test]
fn stale_manifest_is_caught_by_load_time_validation() {
    let dylib = fixture_dylib_path();

    // A manifest with the first two macros swapped: the fingerprint still
    // matches the real dylib, so discovery succeeds — but the macro indices
    // no longer match the dylib's export order.
    let mut manifest = fixture_manifest(&dylib, "test_macro");
    manifest.macros.swap(0, 1);
    manifest.dylib.path = dylib.clone();
    let dir = std::env::temp_dir().join(format!("yelang-disc-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_path = dir.join("stale.ypm.json");
    manifest.write(&manifest_path).unwrap();

    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    runtime
        .discover(&ProcMacroSource::Manifest(manifest_path))
        .expect("discovery itself succeeds; only the indices are wrong");

    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = make_answer!();
        }
    "#,
    );
    let mut expander = MacroExpander::new(&interner).with_proc_macro_runtime(runtime);
    let result = expander.expand(&program);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.to_string().contains("does not match its manifest")),
        "expected load-time validation error, got {:?}",
        result.errors
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// --- Namespaces and duplicates ---------------------------------------------

#[test]
fn duplicate_name_and_kind_is_rejected_at_registry_level() {
    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let id_a = runtime.resolver_mut().registry_mut().register(
        "make_answer".to_string(),
        ProcMacroKind::FunctionLike,
        0,
        "/lib/a.dylib".to_string(),
        "crate_a".to_string(),
    );
    let err = {
        let registry = runtime.resolver_mut().registry_mut();
        // Simulate what `register_all` would have done before any
        // registration, finding the duplicate and bailing out.
        if registry
            .find("make_answer", ProcMacroKind::FunctionLike)
            .is_some()
        {
            Err(DiscoveryError::DuplicateMacro {
                name: "make_answer".to_string(),
                kind: ProcMacroKind::FunctionLike,
                first_crate: "crate_a".to_string(),
                second_crate: "crate_b".to_string(),
            })
        } else {
            Ok(())
        }
    };
    assert!(
        matches!(
            err,
            Err(DiscoveryError::DuplicateMacro {
                first_crate: _,
                second_crate: _,
                ..
            })
        ),
        "expected DuplicateMacro"
    );
    // The first def is still there.
    assert!(runtime.resolver().registry().get(id_a).is_some());
}

#[test]
fn duplicate_library_for_two_crates_is_rejected() {
    let manifest_a = write_fixture_manifest("crate_a");
    let manifest_b = write_fixture_manifest("crate_b");

    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    runtime
        .discover(&ProcMacroSource::Manifest(manifest_a))
        .expect("first crate discovers fine");

    let error = runtime
        .discover(&ProcMacroSource::Manifest(manifest_b))
        .expect_err("same dylib registered twice must be rejected");
    assert!(
        matches!(error, DiscoveryError::DuplicateLibrary { .. }),
        "expected DuplicateLibrary, got {error:?}"
    );

    // All-or-nothing: the rejected source registered nothing.
    assert_eq!(registry_count(&runtime), FIXTURE_MACROS.len());
}

#[test]
fn same_name_in_different_kind_namespaces_coexists() {
    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let registry = runtime.resolver_mut().registry_mut();
    registry.register(
        "make_answer".to_string(),
        ProcMacroKind::FunctionLike,
        0,
        "/lib/a.dylib".to_string(),
        "crate_a".to_string(),
    );
    registry.register(
        "make_answer".to_string(),
        ProcMacroKind::Derive,
        0,
        "/lib/b.dylib".to_string(),
        "crate_b".to_string(),
    );

    let resolver = runtime.resolver();
    let fn_like = resolver
        .resolve("make_answer", ProcMacroKind::FunctionLike)
        .unwrap();
    let derive = resolver
        .resolve("make_answer", ProcMacroKind::Derive)
        .unwrap();
    assert_eq!(fn_like.crate_name, "crate_a");
    assert_eq!(derive.crate_name, "crate_b");
}

// --- Batch discovery and driver API -----------------------------------------

#[test]
fn discover_all_collects_errors_and_still_registers_good_sources() {
    let good_dylib = fixture_dylib_path();
    let good_manifest = write_fixture_manifest("crate_good");

    // Bad source: a copy of the dylib with one byte changed, so its manifest
    // (computed on the original) fails the hash check. A different path means
    // it is not a DuplicateLibrary.
    let dir = std::env::temp_dir().join(format!("yelang-disc-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bad_copy = dir.join(format!(
        "{}bad_macro{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let mut bytes = std::fs::read(&good_dylib).unwrap();
    // Flip a byte that is not a metadata/header boundary: mid-file.
    let idx = bytes.len() / 2;
    bytes[idx] = bytes[idx].wrapping_add(1);
    std::fs::write(&bad_copy, bytes).unwrap();

    let mut bad_manifest = fixture_manifest(&good_dylib, "crate_bad");
    bad_manifest.dylib.path = bad_copy.clone();
    let bad_path = dir.join("bad.ypm.json");
    bad_manifest.write(&bad_path).unwrap();

    let client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let report = runtime.discover_all(&[
        ProcMacroSource::Manifest(bad_path),
        ProcMacroSource::Manifest(good_manifest),
        ProcMacroSource::Manifest(dir.join("does_not_exist.ypm.json")),
    ]);

    assert_eq!(report.errors.len(), 2, "report: {report:?}");
    assert_eq!(report.discovered.len(), 1);
    assert_eq!(report.discovered[0].crate_name, "crate_good");
    assert!(!report.is_success());

    // The good source is fully usable.
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

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn spawn_default_finds_server_via_env_var() {
    // Edition 2024: environment mutation is unsafe; this is the only test in
    // the process that touches this variable.
    unsafe { std::env::set_var("YELANG_PROC_MACRO_SERVER", server_path()) };
    let client = ProcMacroClient::spawn_default();
    unsafe { std::env::remove_var("YELANG_PROC_MACRO_SERVER") };
    client.expect("spawn_default via YELANG_PROC_MACRO_SERVER");
}

#[test]
fn driver_api_expands_program_with_proc_macros() {
    let manifest = write_fixture_manifest("test_macro");
    let (runtime, _) = runtime_with(ProcMacroSource::Manifest(manifest));
    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = make_answer!();
        }
    "#,
    );

    let result = yelang_macro::expand_program_with_proc_macros(&program, &interner, runtime);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let ItemKind::Fn(func) = &result.program.items[0].kind else {
        panic!("expected fn")
    };
    assert_eq!(func.body.statements.len(), 1);
}
