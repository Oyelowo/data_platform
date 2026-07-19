//! End-to-end Yelang compiler driver.
//!
//! This crate orchestrates the full compiler pipeline from raw `.ye` source to
//! an executed query result. It is intended for tests, REPLs, and any tool that
//! wants to run Yelang without manually wiring lexer → parser → resolver → HIR
//! → typechecker → QIR → planner → executor.
//!
//! # Example
//!
//! ```
//! use yelang_driver::Driver;
//!
//! let result = Driver::new()
//!     .run(r#"
//!         fn main() {
//!             let xs = [1, 2, 3];
//!             let _ = select x + 1 from xs@x;
//!         }
//!     "#)
//!     .expect("query should run");
//! ```

pub mod driver;
pub mod error;
pub mod stdlib;

pub use driver::{CompiledCrate, Driver};
pub use error::{DriverError, Result};
