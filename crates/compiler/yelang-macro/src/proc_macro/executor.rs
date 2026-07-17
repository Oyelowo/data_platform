//! In-process execution of procedural macros.
//!
//! This is used for testing and bootstrapping until the out-of-process server
//! and dylib loading are fully implemented.

use yelang_proc_macro::{Diagnostic, TokenStream};
use yelang_proc_macro_bridge::protocol::ProcMacroKind;

/// A procedural macro implemented in-process.
pub trait InProcessProcMacro: Send + Sync {
    fn kind(&self) -> ProcMacroKind;
    fn name(&self) -> &str;

    /// Optional span pointing to the macro definition. When present it is sent
    /// to the proc-macro API as `Span::def_site()`; otherwise the call site is
    /// used as a fallback.
    fn def_site_span(&self) -> Option<yelang_lexer::Span> {
        None
    }

    fn expand_fn_like(&self, input: TokenStream) -> (TokenStream, Vec<Diagnostic>);
    fn expand_attr(&self, args: TokenStream, item: TokenStream) -> (TokenStream, Vec<Diagnostic>);
    fn expand_derive(&self, item: TokenStream) -> (TokenStream, Vec<Diagnostic>);
}

/// Executor that runs procedural macros without a separate server process.
#[derive(Default)]
pub struct InProcessExecutor {
    macros: Vec<Box<dyn InProcessProcMacro>>,
}

impl InProcessExecutor {
    pub fn new() -> Self {
        Self { macros: Vec::new() }
    }

    pub fn register(&mut self, mac: Box<dyn InProcessProcMacro>) {
        self.macros.push(mac);
    }

    pub fn find(&self, name: &str) -> Option<&dyn InProcessProcMacro> {
        self.macros
            .iter()
            .find(|m| m.name() == name)
            .map(|m| m.as_ref())
    }
}
