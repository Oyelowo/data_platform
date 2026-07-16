//! Shared support for tests that exercise the `test_macro` fixture dylib
//! (`yelang-proc-macro-test-fixture`): locating the server and the dylib,
//! emitting manifests with a live fingerprint, and parsing source programs.

use std::path::{Path, PathBuf};

use yelang_interner::Interner;
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
    ("panic", ProcMacroKind::FunctionLike),
];

pub fn server_path() -> Option<String> {
    option_env!("CARGO_BIN_EXE_yelang-proc-macro-server").map(String::from)
}

pub fn fixture_dylib_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.parent().unwrap().join("target"));
    let file_name = format!(
        "{}test_macro{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    target_dir.join("debug").join(file_name)
}

/// Path to the fixture dylib, or `None` (with a note) if it has not been
/// built yet — tests skip gracefully in that case.
pub fn require_fixture_dylib() -> Option<PathBuf> {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!(
            "fixture dylib not found at {}; skipping test (build \
             yelang-proc-macro-test-fixture first)",
            dylib.display()
        );
        return None;
    }
    Some(dylib)
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

/// Write a manifest for the fixture dylib into the dylib's own directory and
/// return its path.
pub fn write_fixture_manifest(crate_name: &str) -> Option<PathBuf> {
    let dylib = require_fixture_dylib()?;
    let manifest = fixture_manifest(&dylib, crate_name);
    let path = dylib.with_file_name(format!("{crate_name}.ypm.json"));
    manifest.write(&path).expect("write fixture manifest");
    Some(path)
}

pub fn parse_program(src: &str) -> (yelang_ast::Program, Interner) {
    let mut interner = Interner::new();
    let mut stream = yelang_ast::TokenKind::tokenize(src, &mut interner).unwrap();
    let program = stream.parse::<yelang_ast::Program>().unwrap();
    (program, interner)
}
