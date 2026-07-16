//! Integration tests for Phase 7.5 cross-crate proc-macro discovery:
//! manifest-based discovery, server introspection, freshness validation,
//! duplicate detection, and the driver-facing API.

mod proc_macro_fixture;

use std::path::{Path, PathBuf};

use proc_macro_fixture::{
    FIXTURE_MACROS, fixture_manifest, parse_program, require_fixture_dylib, server_path,
    write_fixture_manifest,
};
use yelang_ast::ItemKind;
use yelang_macro::MacroExpander;
use yelang_macro::proc_macro::{
    DiscoveredCrate, DiscoveryError, ManifestMacro, ProcMacroClient, ProcMacroKind,
    ProcMacroRegistry, ProcMacroResolver, ProcMacroRuntime, ProcMacroSource, Provenance,
};

fn runtime_with(source: ProcMacroSource) -> Option<(ProcMacroRuntime, DiscoveredCrate)> {
    let server = server_path()?;
    require_fixture_dylib()?;
    let client = ProcMacroClient::spawn(&server).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let discovered = runtime.discover(&source).expect("discovery should succeed");
    Some((runtime, discovered))
}

fn registry_count(runtime: &ProcMacroRuntime) -> usize {
    runtime.resolver().registry().iter().count()
}

// --- Manifest discovery -------------------------------------------------

#[test]
fn manifest_discovery_registers_every_macro_without_loading() {
    let Some(manifest) = write_fixture_manifest("test_macro") else {
        return;
    };
    let Some((runtime, discovered)) = runtime_with(ProcMacroSource::Manifest(manifest)) else {
        return;
    };

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
    let Some(manifest) = write_fixture_manifest("test_macro") else {
        return;
    };
    let Some((runtime, _)) = runtime_with(ProcMacroSource::Manifest(manifest)) else {
        return;
    };
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
    let Some(dylib) = require_fixture_dylib() else {
        return;
    };
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

    let Some((runtime, _)) = runtime_with(ProcMacroSource::Manifest(manifest_path)) else {
        return;
    };
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
    let Some(dylib) = require_fixture_dylib() else {
        return;
    };
    let Some((runtime, discovered)) = runtime_with(ProcMacroSource::Dylib(dylib.clone())) else {
        return;
    };

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
    let Some(dylib) = require_fixture_dylib() else {
        return;
    };

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

    let Some((runtime, discovered)) = runtime_with(ProcMacroSource::Dylib(dylib_copy)) else {
        return;
    };
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
    doctor: impl FnOnce(&mut yelang_macro::proc_macro::ProcMacroCrateManifest),
) -> Option<DiscoveryError> {
    let dylib = require_fixture_dylib()?;
    let server = server_path()?;
    let mut manifest = fixture_manifest(&dylib, "test_macro");
    // Absolute dylib path: the doctored manifest lives in a temp dir. Set
    // before doctoring so a doctor can deliberately re-point it.
    manifest.dylib.path = dylib.clone();
    doctor(&mut manifest);

    let dir = std::env::temp_dir().join(format!("yelang-disc-doctored-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_path = dir.join("doctored.ypm.json");
    manifest.write(&manifest_path).unwrap();

    let client = ProcMacroClient::spawn(&server).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let error = runtime
        .discover(&ProcMacroSource::Manifest(manifest_path))
        .expect_err("doctored manifest must be rejected");
    let _ = std::fs::remove_dir_all(&dir);
    Some(error)
}

#[test]
fn tampered_hash_is_rejected() {
    let Some(error) = discover_with_doctored_manifest(|m| {
        m.dylib.content_hash = "blake3:deadbeef".to_string();
    }) else {
        return;
    };
    assert!(
        matches!(error, DiscoveryError::HashMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn tampered_size_is_rejected() {
    let Some(error) = discover_with_doctored_manifest(|m| {
        m.dylib.size += 1;
    }) else {
        return;
    };
    assert!(
        matches!(error, DiscoveryError::SizeMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn wrong_protocol_version_is_rejected_before_loading() {
    let Some(error) = discover_with_doctored_manifest(|m| {
        m.protocol_version += 1;
    }) else {
        return;
    };
    assert!(
        matches!(error, DiscoveryError::ProtocolMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn wrong_host_triple_is_rejected_before_loading() {
    let Some(error) = discover_with_doctored_manifest(|m| {
        m.host_triple = "definitely-not-the-host-triple".to_string();
    }) else {
        return;
    };
    assert!(
        matches!(error, DiscoveryError::TripleMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn missing_dylib_is_reported_at_discovery() {
    let Some(error) = discover_with_doctored_manifest(|m| {
        m.dylib.path = PathBuf::from("/nonexistent/libgone.dylib");
    }) else {
        return;
    };
    assert!(matches!(error, DiscoveryError::Io { .. }), "got {error:?}");
}

#[test]
fn stale_manifest_is_caught_by_load_time_validation() {
    let Some(dylib) = require_fixture_dylib() else {
        return;
    };
    let Some(server) = server_path() else { return };

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

    let client = ProcMacroClient::spawn(&server).expect("spawn server");
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
fn duplicate_name_and_kind_from_two_crates_is_rejected() {
    let Some(manifest_a) = write_fixture_manifest("crate_a") else {
        return;
    };
    let Some(manifest_b) = write_fixture_manifest("crate_b") else {
        return;
    };
    let Some(server) = server_path() else { return };

    let client = ProcMacroClient::spawn(&server).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    runtime
        .discover(&ProcMacroSource::Manifest(manifest_a))
        .expect("first crate discovers fine");

    let error = runtime
        .discover(&ProcMacroSource::Manifest(manifest_b))
        .expect_err("overlapping exports must be rejected");
    match error {
        DiscoveryError::DuplicateMacro {
            name,
            kind,
            first_crate,
            second_crate,
        } => {
            assert_eq!(name, "make_answer");
            assert_eq!(kind, ProcMacroKind::FunctionLike);
            assert_eq!(first_crate, "crate_a");
            assert_eq!(second_crate, "crate_b");
        }
        other => panic!("expected DuplicateMacro, got {other:?}"),
    }

    // All-or-nothing: the rejected source registered nothing.
    assert_eq!(registry_count(&runtime), FIXTURE_MACROS.len());
}

#[test]
fn same_name_in_different_kind_namespaces_coexists() {
    let Some(manifest_a) = write_fixture_manifest("crate_a") else {
        return;
    };
    let Some(dylib) = require_fixture_dylib() else {
        return;
    };
    let Some(server) = server_path() else { return };

    // crate_b exports a *derive* also named `make_answer` — a different
    // namespace than crate_a's function-like `make_answer`. (Registry-level
    // test: crate_b's manifest is intentionally partial, so its macro is
    // never expanded — load-time validation would reject it, by design.)
    let mut manifest_b = fixture_manifest(&dylib, "crate_b");
    manifest_b.macros = vec![ManifestMacro {
        name: "make_answer".to_string(),
        kind: ProcMacroKind::Derive,
    }];
    manifest_b.dylib.path = dylib.clone();
    let dir = std::env::temp_dir().join(format!("yelang-disc-kinds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_b_path = dir.join("crate_b.ypm.json");
    manifest_b.write(&manifest_b_path).unwrap();

    let client = ProcMacroClient::spawn(&server).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    runtime
        .discover(&ProcMacroSource::Manifest(manifest_a))
        .expect("crate_a discovers fine");
    runtime
        .discover(&ProcMacroSource::Manifest(manifest_b_path))
        .expect("same name in a different kind namespace is allowed");

    let resolver = runtime.resolver();
    let fn_like = resolver
        .resolve("make_answer", ProcMacroKind::FunctionLike)
        .unwrap();
    let derive = resolver
        .resolve("make_answer", ProcMacroKind::Derive)
        .unwrap();
    assert_eq!(fn_like.crate_name, "crate_a");
    assert_eq!(derive.crate_name, "crate_b");

    let _ = std::fs::remove_dir_all(&dir);
}

// --- Batch discovery and driver API -----------------------------------------

#[test]
fn discover_all_collects_errors_and_still_registers_good_sources() {
    let Some(dylib) = require_fixture_dylib() else {
        return;
    };
    let Some(server) = server_path() else { return };

    let good = write_fixture_manifest("crate_good").unwrap();

    let dir = std::env::temp_dir().join(format!("yelang-disc-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut bad = fixture_manifest(&dylib, "crate_bad");
    bad.dylib.content_hash = "blake3:bad".to_string();
    bad.dylib.path = dylib.clone();
    let bad_path = dir.join("bad.ypm.json");
    bad.write(&bad_path).unwrap();

    let client = ProcMacroClient::spawn(&server).expect("spawn server");
    let mut runtime =
        ProcMacroRuntime::new(client, ProcMacroResolver::new(ProcMacroRegistry::new()));
    let report = runtime.discover_all(&[
        ProcMacroSource::Manifest(bad_path),
        ProcMacroSource::Manifest(good),
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
    let Some(server) = server_path() else { return };
    // Edition 2024: environment mutation is unsafe; this is the only test in
    // the process that touches this variable.
    unsafe { std::env::set_var("YELANG_PROC_MACRO_SERVER", &server) };
    let client = ProcMacroClient::spawn_default();
    unsafe { std::env::remove_var("YELANG_PROC_MACRO_SERVER") };
    client.expect("spawn_default via YELANG_PROC_MACRO_SERVER");
}

#[test]
fn driver_api_expands_program_with_proc_macros() {
    let Some(manifest) = write_fixture_manifest("test_macro") else {
        return;
    };
    let Some((runtime, _)) = runtime_with(ProcMacroSource::Manifest(manifest)) else {
        return;
    };
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
