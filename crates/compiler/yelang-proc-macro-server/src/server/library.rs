//! Loaded proc-macro library representation.

use std::sync::Arc;

use yelang_proc_macro_bridge::protocol::{MacroDescriptor, ProcMacroKind};

/// A macro function that can be invoked by the server.
pub trait ProcMacro: Send + Sync {
    fn kind(&self) -> ProcMacroKind;
    fn name(&self) -> &str;

    /// Invoke the macro. Returns the output token stream and any diagnostics.
    fn expand_fn_like(&self, input: TokenStreamAndContext) -> MacroResult;
    fn expand_attr(&self, args: TokenStreamAndContext, item: TokenStreamAndContext) -> MacroResult;
    fn expand_derive(&self, item: TokenStreamAndContext) -> MacroResult;
}

pub type TokenStreamAndContext = (yelang_macro_core::TokenStream, yelang_macro_core::Span);
pub type MacroResult = Result<yelang_macro_core::TokenStream, String>;

/// A library loaded into the server.
pub struct LoadedLibrary {
    pub path: String,
    pub macros: Vec<Arc<dyn ProcMacro>>,
    pub descriptors: Vec<MacroDescriptor>,
}

impl LoadedLibrary {
    pub fn new(path: String, macros: Vec<Arc<dyn ProcMacro>>) -> Self {
        let descriptors = macros
            .iter()
            .map(|m| MacroDescriptor {
                name: m.name().to_string(),
                kind: m.kind(),
            })
            .collect();
        Self {
            path,
            macros,
            descriptors,
        }
    }

    pub fn get_macro(&self, index: usize) -> Option<&Arc<dyn ProcMacro>> {
        self.macros.get(index)
    }
}
