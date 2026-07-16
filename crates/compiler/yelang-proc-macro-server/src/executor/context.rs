//! Per-invocation context for proc macros.

use std::cell::RefCell;

use yelang_proc_macro::Diagnostic;

/// Context available during a single macro expansion.
#[derive(Debug, Default)]
pub struct MacroContext {
    pub diagnostics: Vec<Diagnostic>,
}

impl MacroContext {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }
}

thread_local! {
    static CONTEXT: RefCell<Option<MacroContext>> = RefCell::new(None);
}

pub fn enter<R>(f: impl FnOnce() -> R) -> (R, Vec<Diagnostic>) {
    CONTEXT.with(|c| {
        *c.borrow_mut() = Some(MacroContext::new());
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let ctx = CONTEXT.with(|c| c.borrow_mut().take().unwrap_or_default());
    let diagnostics = ctx.diagnostics;
    match result {
        Ok(r) => (r, diagnostics),
        Err(e) => std::panic::resume_unwind(e),
    }
}

pub fn with_diagnostics<R>(f: impl FnOnce(&mut Vec<Diagnostic>) -> R) -> R {
    CONTEXT.with(|c| {
        let mut borrow = c.borrow_mut();
        let ctx = borrow.as_mut().expect("no active macro context");
        f(&mut ctx.diagnostics)
    })
}
