//! Panic handling for macro invocations.

use std::panic;

/// Convert a panic payload into a human-readable message.
pub fn payload_to_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "proc macro panicked".to_string()
    }
}

/// Install a panic hook that records the panic without aborting the process.
pub fn install_hook() -> impl FnOnce() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {
        // Suppress default panic output; the server reports it as a Response.
    }));
    move || {
        panic::set_hook(previous);
    }
}
