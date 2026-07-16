//! Shared support for tests that exercise the `test_macro` fixture dylib
//! (`yelang-proc-macro-test-fixture`): locating the server and the dylib,
//! (re)building the fixture if necessary, emitting manifests with a live
//! fingerprint, and parsing source programs.
//!
//! This module lives in `yelang-proc-macro-server/tests` because the test
//! suite needs the server binary (`env!("CARGO_BIN_EXE_...")`) and the
//! fixture cdylib, neither of which is available to tests in `yelang-macro`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use yelang_macro::proc_macro::{
    DylibSection, HOST_TRIPLE, MANIFEST_FORMAT_VERSION, ManifestMacro, ProcMacroCrateManifest,
    ProcMacroKind, fingerprint_dylib,
};
use yelang_proc_macro_bridge::protocol::CURRENT_PROTOCOL_VERSION;

/// The fixture dylib's export order (see the fixture's `MACROS` array). A
/// manifest's `macros` list must match this order: position is `macro_index`.
pub const FIXTURE_MACROS: &[(&str, ProcMacroKind)] = &[
    ("make_answer", ProcMacroKind::FunctionLike),
    ("trace", ProcMacroKind::Attribute),
    ("answer", ProcMacroKind::Derive),
    ("generate_const", ProcMacroKind::Derive),
    ("emit_warning", ProcMacroKind::FunctionLike),
    ("explode", ProcMacroKind::FunctionLike), // renamed to avoid shadowing builtin `panic!`
];

pub fn server_path() -> &'static str {
    env!("CARGO_BIN_EXE_yelang-proc-macro-server")
}

fn workspace_target_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // This crate is one level under the workspace root.
    manifest_dir.parent().unwrap().join("target")
}

fn possible_dylib_paths() -> Vec<PathBuf> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(workspace_target_dir);
    let file_name = format!(
        "{}test_macro{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    vec![
        target_dir.join("debug").join(&file_name),
        target_dir.join(HOST_TRIPLE).join("debug").join(&file_name),
    ]
}

/// Build the fixture cdylib by invoking Cargo, then return the path where the
/// artifact was produced.
pub fn build_fixture() -> PathBuf {
    let output = Command::new("cargo")
        .args([
            "build",
            "-p",
            "yelang-proc-macro-test-fixture",
            "--message-format=short",
        ])
        .output()
        .expect("spawn cargo build for fixture");
    if !output.status.success() {
        panic!(
            "failed to build yelang-proc-macro-test-fixture:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Re-use the search logic now that the artifact should exist.
    possible_dylib_paths()
        .into_iter()
        .find(|p| p.exists())
        .expect("fixture dylib not found after cargo build")
}

/// Path to the compiled fixture dylib, building it on demand if necessary.
pub fn fixture_dylib_path() -> PathBuf {
    possible_dylib_paths()
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(build_fixture)
}

/// A manifest describing the fixture dylib, with a freshly computed
/// fingerprint and the dylib referenced by file name (relative to the
/// manifest's location).
pub fn fixture_manifest(dylib: &Path, crate_name: &str) -> ProcMacroCrateManifest {
    let (content_hash, size) = fingerprint_dylib(dylib).expect("fingerprint fixture dylib");
    ProcMacroCrateManifest {
        format_version: MANIFEST_FORMAT_VERSION,
        crate_name: crate_name.to_string(),
        crate_version: "0.1.0".to_string(),
        host_triple: HOST_TRIPLE.to_string(),
        protocol_version: CURRENT_PROTOCOL_VERSION,
        dylib: DylibSection {
            path: PathBuf::from(dylib.file_name().unwrap()),
            content_hash,
            size,
        },
        macros: FIXTURE_MACROS
            .iter()
            .map(|(name, kind)| ManifestMacro {
                name: name.to_string(),
                kind: *kind,
            })
            .collect(),
    }
}

static MANIFEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write a manifest for the fixture dylib into an isolated temp directory
/// (unique to this invocation) and return its path. Uses an absolute dylib
/// path so the manifest is self-contained and concurrent tests do not race
/// over files in `target/`.
pub fn write_fixture_manifest(crate_name: &str) -> PathBuf {
    let dylib = fixture_dylib_path();
    let mut manifest = fixture_manifest(&dylib, crate_name);
    // Make the manifest self-contained: the dylib lives under `target/`, while
    // the manifest is emitted into an isolated temp directory per call.
    manifest.dylib.path = dylib
        .canonicalize()
        .expect("canonicalize fixture dylib path");
    let counter = MANIFEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "yelang-fixture-manifest-{crate_name}-{}-{counter}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{crate_name}.ypm.json"));
    manifest.write(&path).expect("write fixture manifest");
    path
}

pub fn parse_program(src: &str) -> (yelang_ast::Program, yelang_interner::Interner) {
    let mut interner = yelang_interner::Interner::new();
    let mut stream = yelang_ast::TokenKind::tokenize(src, &mut interner).unwrap();
    let program = stream.parse::<yelang_ast::Program>().unwrap();
    (program, interner)
}
