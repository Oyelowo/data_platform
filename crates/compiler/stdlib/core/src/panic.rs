//! Panic infrastructure for Yelang.
//!
//! `panic!()` and `assert!()` lower to calls into this module.

/// The default panic handler.
pub fn default_panic_handler(msg: &str) -> ! {
    eprintln!("panic: {}", msg);
    std::process::exit(101);
}
