//! Proc-macro crate manifests (`*.ypm.json`).
//!
//! A manifest is a JSON sidecar emitted by the build system next to a
//! compiled proc-macro dylib. It records everything discovery needs to know
//! about the crate — name, version, host triple, protocol version, dylib
//! fingerprint, and the exported macros in dylib export order — so that the
//! compiler can register a crate's macros **without loading any code**
//! (mirroring how rustc reads `proc_macro_data` from crate metadata and only
//! `dlopen`s at first expansion).

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use yelang_proc_macro_bridge::protocol::{CURRENT_PROTOCOL_VERSION, ProcMacroKind};

use super::discovery::DiscoveryError;

/// Manifest schema version. Bump on any breaking change to the format.
pub const MANIFEST_FORMAT_VERSION: u32 = 1;

/// Extension (including the leading dot) of manifest sidecar files.
pub const MANIFEST_EXTENSION: &str = "ypm.json";

/// Manifests are small by definition; refuse to parse anything larger.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// The target triple the compiler itself was built for. Proc macros run on
/// the host, so this is the triple proc-macro dylibs must match.
pub const HOST_TRIPLE: &str = env!("YELANG_HOST_TRIPLE");

/// A proc-macro crate manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcMacroCrateManifest {
    pub format_version: u32,
    pub crate_name: String,
    pub crate_version: String,
    pub host_triple: String,
    pub protocol_version: u32,
    pub dylib: DylibSection,
    /// Exported macros **in dylib export order**: the position in this vec is
    /// the `macro_index` used in expansion requests.
    pub macros: Vec<ManifestMacro>,
}

/// Location and content fingerprint of the compiled dylib.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DylibSection {
    /// Relative paths resolve against the manifest's directory.
    pub path: PathBuf,
    /// `"blake3:<hex>"` of the dylib bytes.
    pub content_hash: String,
    pub size: u64,
}

/// One exported macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestMacro {
    pub name: String,
    pub kind: ProcMacroKind,
}

impl ProcMacroCrateManifest {
    /// Read and structurally validate a manifest.
    ///
    /// Structural validation covers everything that does not depend on the
    /// environment: schema version, non-empty fields, non-empty macro list,
    /// and no duplicate `(name, kind)` within the manifest.
    /// Environment checks (dylib presence, fingerprint, triple, protocol) are
    /// performed by [`Self::validate_environment`].
    pub fn read(path: &Path) -> Result<Self, DiscoveryError> {
        let metadata = std::fs::metadata(path).map_err(|e| DiscoveryError::Io {
            op: "stat manifest",
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        // Read up to MAX_MANIFEST_BYTES + 1 bytes so a file that grew between
        // the stat and the read is still bounded, rather than parsed.
        let file = File::open(path).map_err(|e| DiscoveryError::Io {
            op: "open manifest",
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let mut text = String::with_capacity(MAX_MANIFEST_BYTES.min(metadata.len()) as usize);
        let mut limited = file.take(MAX_MANIFEST_BYTES + 1);
        limited
            .read_to_string(&mut text)
            .map_err(|e| DiscoveryError::Io {
                op: "read manifest",
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;

        if text.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(DiscoveryError::ManifestTooLarge {
                path: path.to_path_buf(),
                size: text.len() as u64,
            });
        }

        let manifest: Self =
            serde_json::from_str(&text).map_err(|e| DiscoveryError::ManifestParse {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
        manifest.validate_structure(path)?;
        Ok(manifest)
    }

    /// Serialize as pretty JSON (build systems and tests emit manifests).
    /// Writes atomically: temp file in the target directory, then `rename`,
    /// so concurrent readers never see a partially written manifest.
    pub fn write(&self, path: &Path) -> Result<(), DiscoveryError> {
        let text =
            serde_json::to_string_pretty(self).map_err(|e| DiscoveryError::InvalidManifest {
                path: path.to_path_buf(),
                reason: format!("failed to serialize manifest: {e}"),
            })?;

        let temp = path.with_extension("ypm.json.tmp");
        let mut file = File::create(&temp).map_err(|e| DiscoveryError::Io {
            op: "create temp manifest",
            path: temp.clone(),
            message: e.to_string(),
        })?;
        file.write_all(text.as_bytes()).map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            DiscoveryError::Io {
                op: "write temp manifest",
                path: temp.clone(),
                message: e.to_string(),
            }
        })?;
        drop(file);
        std::fs::rename(&temp, path).map_err(|e| DiscoveryError::Io {
            op: "rename manifest",
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    fn validate_structure(&self, path: &Path) -> Result<(), DiscoveryError> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(DiscoveryError::UnsupportedFormatVersion {
                path: path.to_path_buf(),
                found: self.format_version,
                expected: MANIFEST_FORMAT_VERSION,
            });
        }
        for (field, value) in [
            ("crate_name", &self.crate_name),
            ("crate_version", &self.crate_version),
        ] {
            if value.trim().is_empty() {
                return Err(DiscoveryError::InvalidManifest {
                    path: path.to_path_buf(),
                    reason: format!("{field} must not be empty"),
                });
            }
        }
        if self.macros.is_empty() {
            return Err(DiscoveryError::EmptyLibrary {
                crate_name: self.crate_name.clone(),
            });
        }
        if self.macros.iter().any(|m| m.name.trim().is_empty()) {
            return Err(DiscoveryError::InvalidManifest {
                path: path.to_path_buf(),
                reason: "macro name must not be empty".to_string(),
            });
        }
        let mut seen = Vec::new();
        for m in &self.macros {
            if seen.iter().any(|(n, k)| n == &m.name && *k == m.kind) {
                return Err(DiscoveryError::InvalidManifest {
                    path: path.to_path_buf(),
                    reason: format!(
                        "duplicate {kind:?} macro `{name}` within the same manifest",
                        kind = m.kind,
                        name = m.name
                    ),
                });
            }
            seen.push((m.name.clone(), m.kind));
        }
        Ok(())
    }

    /// Absolute path of the dylib. Relative manifest paths resolve against
    /// the manifest's directory.
    pub fn dylib_path(&self, manifest_path: &Path) -> PathBuf {
        if self.dylib.path.is_absolute() {
            self.dylib.path.clone()
        } else {
            manifest_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&self.dylib.path)
        }
    }

    /// Validate the manifest against the environment: dylib presence, size,
    /// blake3 content hash, protocol version, and host triple.
    ///
    /// Returns the canonicalized dylib path on success. No code is loaded.
    pub fn validate_environment(&self, manifest_path: &Path) -> Result<PathBuf, DiscoveryError> {
        if self.protocol_version != CURRENT_PROTOCOL_VERSION {
            return Err(DiscoveryError::ProtocolMismatch {
                crate_name: self.crate_name.clone(),
                found: self.protocol_version,
                expected: CURRENT_PROTOCOL_VERSION,
            });
        }
        if self.host_triple != HOST_TRIPLE {
            return Err(DiscoveryError::TripleMismatch {
                crate_name: self.crate_name.clone(),
                manifest: self.host_triple.clone(),
                host: HOST_TRIPLE.to_string(),
            });
        }

        let dylib_path = self.dylib_path(manifest_path);
        let (hash, size) = fingerprint_dylib(&dylib_path)?;
        if size != self.dylib.size {
            return Err(DiscoveryError::SizeMismatch {
                path: dylib_path,
                expected: self.dylib.size,
                found: size,
            });
        }
        if hash != self.dylib.content_hash {
            return Err(DiscoveryError::HashMismatch { path: dylib_path });
        }

        std::fs::canonicalize(&dylib_path).map_err(|e| DiscoveryError::Io {
            op: "canonicalize dylib",
            path: dylib_path,
            message: e.to_string(),
        })
    }
}

/// Compute the `("blake3:<hex>", size)` fingerprint of a dylib. Used by build
/// systems (and tests) when emitting manifests, and by discovery when
/// validating them.
///
/// Hashes the file incrementally; for large dylibs this avoids a one-shot
/// allocation of the whole file.
pub fn fingerprint_dylib(path: &Path) -> Result<(String, u64), DiscoveryError> {
    let mut file = File::open(path).map_err(|e| DiscoveryError::Io {
        op: "open dylib",
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let mut hasher = blake3::Hasher::new();
    let mut size = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| DiscoveryError::Io {
            op: "read dylib",
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    let hash = hasher.finalize();
    Ok((format!("blake3:{hash}"), size))
}

/// The sidecar manifest path probed for a dylib:
/// `<dylib dir>/<dylib file stem>.ypm.json`.
pub fn sidecar_manifest_path(dylib_path: &Path) -> PathBuf {
    let stem = dylib_path.file_stem().unwrap_or_default().to_string_lossy();
    dylib_path.with_file_name(format!("{stem}.{MANIFEST_EXTENSION}"))
}

/// Crate name synthesized for an introspected dylib: the file stem with the
/// conventional `lib` prefix stripped (`libtest_macro.dylib` → `test_macro`).
pub fn crate_name_from_dylib_path(dylib_path: &Path) -> String {
    let stem = dylib_path.file_stem().unwrap_or_default().to_string_lossy();
    stem.strip_prefix("lib").unwrap_or(&stem).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_proc_macro_bridge::protocol::CURRENT_PROTOCOL_VERSION;

    fn sample_manifest() -> ProcMacroCrateManifest {
        ProcMacroCrateManifest {
            format_version: MANIFEST_FORMAT_VERSION,
            crate_name: "test_macro".to_string(),
            crate_version: "0.1.0".to_string(),
            host_triple: HOST_TRIPLE.to_string(),
            protocol_version: CURRENT_PROTOCOL_VERSION,
            dylib: DylibSection {
                path: PathBuf::from("libtest_macro.dylib"),
                content_hash: "blake3:00".to_string(),
                size: 1,
            },
            macros: vec![
                ManifestMacro {
                    name: "make_answer".to_string(),
                    kind: ProcMacroKind::FunctionLike,
                },
                ManifestMacro {
                    name: "trace".to_string(),
                    kind: ProcMacroKind::Attribute,
                },
                ManifestMacro {
                    name: "answer".to_string(),
                    kind: ProcMacroKind::Derive,
                },
            ],
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "yelang-manifest-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("test_macro.ypm.json");
        let manifest = sample_manifest();
        manifest.write(&path).unwrap();
        let read = ProcMacroCrateManifest::read(&path).unwrap();
        assert_eq!(manifest, read);
    }

    #[test]
    fn manifest_rejects_bad_json() {
        let dir = temp_dir("bad-json");
        let path = dir.join("bad.ypm.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(matches!(
            ProcMacroCrateManifest::read(&path),
            Err(DiscoveryError::ManifestParse { .. })
        ));
    }

    #[test]
    fn manifest_rejects_wrong_format_version() {
        let dir = temp_dir("bad-version");
        let path = dir.join("v.ypm.json");
        let mut manifest = sample_manifest();
        manifest.format_version = 99;
        manifest.write(&path).unwrap();
        assert!(matches!(
            ProcMacroCrateManifest::read(&path),
            Err(DiscoveryError::UnsupportedFormatVersion {
                found: 99,
                expected: 1,
                ..
            })
        ));
    }

    #[test]
    fn manifest_rejects_empty_fields() {
        let dir = temp_dir("empty-fields");
        let path = dir.join("e.ypm.json");
        for mutate in [
            (|m: &mut ProcMacroCrateManifest| m.crate_name.clear())
                as fn(&mut ProcMacroCrateManifest),
            |m| m.crate_version.clear(),
            |m| m.macros[0].name.clear(),
        ] {
            let mut manifest = sample_manifest();
            mutate(&mut manifest);
            manifest.write(&path).unwrap();
            assert!(matches!(
                ProcMacroCrateManifest::read(&path),
                Err(DiscoveryError::InvalidManifest { .. })
            ));
        }
    }

    #[test]
    fn manifest_rejects_empty_macros() {
        let dir = temp_dir("empty-macros");
        let path = dir.join("em.ypm.json");
        let mut manifest = sample_manifest();
        manifest.macros.clear();
        manifest.write(&path).unwrap();
        assert!(matches!(
            ProcMacroCrateManifest::read(&path),
            Err(DiscoveryError::EmptyLibrary { .. })
        ));
    }

    #[test]
    fn manifest_rejects_intra_manifest_duplicate() {
        let dir = temp_dir("dup");
        let path = dir.join("dup.ypm.json");
        let mut manifest = sample_manifest();
        manifest.macros.push(ManifestMacro {
            name: "make_answer".to_string(),
            kind: ProcMacroKind::FunctionLike,
        });
        manifest.write(&path).unwrap();
        assert!(matches!(
            ProcMacroCrateManifest::read(&path),
            Err(DiscoveryError::InvalidManifest { .. })
        ));
    }

    #[test]
    fn manifest_rejects_too_large() {
        let dir = temp_dir("too-large");
        let path = dir.join("big.ypm.json");
        std::fs::write(&path, vec![b' '; (MAX_MANIFEST_BYTES + 1) as usize]).unwrap();
        assert!(matches!(
            ProcMacroCrateManifest::read(&path),
            Err(DiscoveryError::ManifestTooLarge { .. })
        ));
    }

    #[test]
    fn dylib_path_resolves_relative_against_manifest_dir() {
        let manifest = sample_manifest();
        let manifest_path = Path::new("/crates/pm/test_macro.ypm.json");
        assert_eq!(
            manifest.dylib_path(manifest_path),
            PathBuf::from("/crates/pm/libtest_macro.dylib")
        );

        let mut absolute = sample_manifest();
        absolute.dylib.path = PathBuf::from("/elsewhere/libtest_macro.dylib");
        assert_eq!(
            absolute.dylib_path(manifest_path),
            PathBuf::from("/elsewhere/libtest_macro.dylib")
        );
    }

    #[test]
    fn sidecar_path_uses_dylib_stem() {
        assert_eq!(
            sidecar_manifest_path(Path::new("/out/libtest_macro.dylib")),
            PathBuf::from("/out/libtest_macro.ypm.json")
        );
        assert_eq!(
            sidecar_manifest_path(Path::new("/out/test_macro.dll")),
            PathBuf::from("/out/test_macro.ypm.json")
        );
    }

    #[test]
    fn crate_name_strips_lib_prefix() {
        assert_eq!(
            crate_name_from_dylib_path(Path::new("/out/libtest_macro.dylib")),
            "test_macro"
        );
        assert_eq!(
            crate_name_from_dylib_path(Path::new("/out/test_macro.dll")),
            "test_macro"
        );
    }

    #[test]
    fn fingerprint_is_deterministic_and_content_based() {
        let dir = temp_dir("fingerprint");
        let path = dir.join("libx.dylib");
        std::fs::write(&path, b"fake dylib bytes").unwrap();
        let (hash1, size1) = fingerprint_dylib(&path).unwrap();
        let (hash2, size2) = fingerprint_dylib(&path).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(size1, size2);
        assert_eq!(size1, 16);
        assert!(hash1.starts_with("blake3:"));

        std::fs::write(&path, b"different bytes!").unwrap();
        let (hash3, _) = fingerprint_dylib(&path).unwrap();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn validate_environment_checks_fingerprint_triple_and_protocol() {
        let dir = temp_dir("validate");
        let dylib = dir.join("libtest_macro.dylib");
        std::fs::write(&dylib, b"fake dylib").unwrap();
        let (hash, size) = fingerprint_dylib(&dylib).unwrap();

        let mut manifest = sample_manifest();
        manifest.dylib.content_hash = hash;
        manifest.dylib.size = size;
        let manifest_path = dir.join("test_macro.ypm.json");
        manifest.write(&manifest_path).unwrap();

        let read = ProcMacroCrateManifest::read(&manifest_path).unwrap();
        let canonical = read.validate_environment(&manifest_path).unwrap();
        assert!(canonical.is_absolute());

        // Hash mismatch.
        let mut bad = read.clone();
        bad.dylib.content_hash = "blake3:deadbeef".to_string();
        assert!(matches!(
            bad.validate_environment(&manifest_path),
            Err(DiscoveryError::HashMismatch { .. })
        ));

        // Size mismatch.
        let mut bad = read.clone();
        bad.dylib.size += 1;
        assert!(matches!(
            bad.validate_environment(&manifest_path),
            Err(DiscoveryError::SizeMismatch { .. })
        ));

        // Protocol mismatch.
        let mut bad = read.clone();
        bad.protocol_version += 1;
        assert!(matches!(
            bad.validate_environment(&manifest_path),
            Err(DiscoveryError::ProtocolMismatch { .. })
        ));

        // Triple mismatch.
        let mut bad = read.clone();
        bad.host_triple = "x86_64-unknown-linux-gnu".to_string();
        if bad.host_triple != HOST_TRIPLE {
            assert!(matches!(
                bad.validate_environment(&manifest_path),
                Err(DiscoveryError::TripleMismatch { .. })
            ));
        }
    }

    #[test]
    fn validate_environment_reports_missing_dylib() {
        let dir = temp_dir("missing-dylib");
        let manifest_path = dir.join("m.ypm.json");
        let manifest = sample_manifest();
        manifest.write(&manifest_path).unwrap();
        let read = ProcMacroCrateManifest::read(&manifest_path).unwrap();
        assert!(matches!(
            read.validate_environment(&manifest_path),
            Err(DiscoveryError::Io {
                op: "open dylib",
                ..
            })
        ));
    }
}
