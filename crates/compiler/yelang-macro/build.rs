//! Build script: expose the compiler's own target triple as
//! `YELANG_HOST_TRIPLE`. Procedural macros always run on the host, so the
//! triple the compiler itself is built for is the reference that proc-macro
//! crate manifests are validated against.

fn main() {
    let target = std::env::var("TARGET").expect("TARGET is always set for build scripts");
    println!("cargo:rustc-env=YELANG_HOST_TRIPLE={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
