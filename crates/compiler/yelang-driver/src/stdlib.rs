//! Standard-library loading helpers.

use std::path::{Path, PathBuf};

use crate::error::Result;

/// Names of the `.ye` files that make up the core prelude, in dependency order.
pub const CORE_STDLIB_FILES: &[&str] = &["iter.ye", "aggregate.ye", "aggregate_impls.ye", "query.ye"];

/// Locate the core stdlib directory relative to this crate's manifest.
pub fn core_stdlib_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("../stdlib/core/src");
    dir
}

/// Load the core standard-library source as a single string.
///
/// The files are concatenated in dependency order and separated by newlines so
/// that they behave as if emitted at the root scope. This avoids needing a
/// real module loader for the prelude used by the driver.
pub fn load_core_stdlib() -> Result<String> {
    load_stdlib_from_dir(&core_stdlib_dir())
}

/// Load a stdlib from an arbitrary directory.
pub fn load_stdlib_from_dir(dir: &Path) -> Result<String> {
    let mut out = String::new();
    for name in CORE_STDLIB_FILES {
        let path = dir.join(name);
        let src = std::fs::read_to_string(&path)?;
        out.push_str(&src);
        out.push('\n');
    }
    Ok(out)
}
