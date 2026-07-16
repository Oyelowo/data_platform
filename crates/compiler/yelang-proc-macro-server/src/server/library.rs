//! Loaded proc-macro dynamic-library representation.
//!
//! A proc-macro crate is compiled to a `.so`/`.dll`/`.dylib`. The server loads it
//! with `libloading`, calls the stable C ABI entry point
//! `yelang_proc_macro_entry`, and stores the returned macro descriptors and
//! function pointers. Each expansion serializes the input token stream to a
//! postcard buffer, invokes the appropriate function pointer, and deserializes
//! the returned `WireExpansionResult`.

use std::path::Path;

use libloading::Library;
use yelang_proc_macro_bridge::abi::{
    CURRENT_ABI_VERSION, ENTRY_SYMBOL, YelangAttrMacro, YelangDeriveMacro, YelangFnLikeMacro,
    YelangFreeFn, YelangMacroDescriptor, YelangProcMacroEntry, YelangProcMacroKind,
};
use yelang_proc_macro_bridge::protocol::{MacroDescriptor, ProcMacroKind};

/// A macro exported by a loaded dynamic library, ready to be invoked through the
/// stable serialized ABI.
#[derive(Clone, Copy)]
pub enum LoadedMacro {
    FunctionLike(YelangFnLikeMacro),
    Attribute(YelangAttrMacro),
    Derive(YelangDeriveMacro),
}

/// A library loaded into the server.
pub struct LoadedLibrary {
    pub path: String,
    pub descriptors: Vec<MacroDescriptor>,
    macros: Vec<LoadedMacro>,
    free: YelangFreeFn,
    // The library handle must outlive the function pointers copied out of it.
    _library: Library,
}

impl LoadedLibrary {
    /// Load a proc-macro dynamic library from `path` and validate its ABI.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LibraryLoadError> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        let library = unsafe { Library::new(path_ref) }.map_err(LibraryLoadError::OpenFailed)?;

        let entry: libloading::Symbol<'_, YelangProcMacroEntry> =
            unsafe { library.get(ENTRY_SYMBOL) }.map_err(|e| LibraryLoadError::MissingSymbol {
                name: String::from_utf8_lossy(ENTRY_SYMBOL).to_string(),
                source: e,
            })?;

        let exports = unsafe {
            let ptr = entry(CURRENT_ABI_VERSION);
            ptr.as_ref().ok_or(LibraryLoadError::NullEntry)?
        };

        if exports.abi_version != CURRENT_ABI_VERSION {
            return Err(LibraryLoadError::AbiVersionMismatch {
                expected: CURRENT_ABI_VERSION,
                found: exports.abi_version,
            });
        }

        let free = exports.free;

        let count = exports.macro_count;
        let descriptors_ptr = exports.macros;
        if descriptors_ptr.is_null() {
            return Err(LibraryLoadError::NullDescriptorTable);
        }

        let mut descriptors = Vec::with_capacity(count);
        let mut macros = Vec::with_capacity(count);

        for i in 0..count {
            let desc = unsafe { &*descriptors_ptr.add(i) };
            let name = read_macro_name(desc)?;
            let kind = map_kind(desc.kind);

            descriptors.push(MacroDescriptor { name, kind });
            macros.push(unsafe { load_macro(desc) });
        }

        Ok(Self {
            path: path_str,
            descriptors,
            macros,
            free,
            _library: library,
        })
    }

    /// Return the macro at `index` if it exists.
    pub fn get_macro(&self, index: usize) -> Option<LoadedMacro> {
        self.macros.get(index).copied()
    }

    /// Allocator exported by the dylib; used to free returned buffers.
    pub fn free_fn(&self) -> YelangFreeFn {
        self.free
    }
}

fn read_macro_name(desc: &YelangMacroDescriptor) -> Result<String, LibraryLoadError> {
    if desc.name.is_null() {
        return Err(LibraryLoadError::NullMacroName);
    }
    let bytes = unsafe { std::slice::from_raw_parts(desc.name, desc.name_len) };
    String::from_utf8(bytes.to_vec()).map_err(|_| LibraryLoadError::InvalidMacroName)
}

fn map_kind(kind: YelangProcMacroKind) -> ProcMacroKind {
    match kind {
        YelangProcMacroKind::FunctionLike => ProcMacroKind::FunctionLike,
        YelangProcMacroKind::Attribute => ProcMacroKind::Attribute,
        YelangProcMacroKind::Derive => ProcMacroKind::Derive,
    }
}

unsafe fn load_macro(desc: &YelangMacroDescriptor) -> LoadedMacro {
    match desc.kind {
        YelangProcMacroKind::FunctionLike => LoadedMacro::FunctionLike(desc.invoke.fn_like),
        YelangProcMacroKind::Attribute => LoadedMacro::Attribute(desc.invoke.attr),
        YelangProcMacroKind::Derive => LoadedMacro::Derive(desc.invoke.derive),
    }
}

/// Failure while loading a proc-macro dynamic library.
#[derive(Debug, thiserror::Error)]
pub enum LibraryLoadError {
    #[error("failed to open library: {0}")]
    OpenFailed(#[source] libloading::Error),

    #[error("missing required symbol {name}: {source}")]
    MissingSymbol {
        name: String,
        #[source]
        source: libloading::Error,
    },

    #[error("entry point returned null")]
    NullEntry,

    #[error("ABI version mismatch: expected {expected}, found {found}")]
    AbiVersionMismatch { expected: u32, found: u32 },

    #[error("macro descriptor table is null")]
    NullDescriptorTable,

    #[error("macro name pointer is null")]
    NullMacroName,

    #[error("macro name is not valid UTF-8")]
    InvalidMacroName,
}
