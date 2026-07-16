//! Integration tests for the proc-macro server.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use yelang_proc_macro_bridge::protocol::{
    CURRENT_PROTOCOL_VERSION, ErrorCode, LibraryHandle, MacroDescriptor, ProcMacroKind, Request,
    Response, WireTokenStream,
    serialize::{read_response, write_request},
    token::WireTokenTree,
};

fn server_path() -> &'static str {
    env!("CARGO_BIN_EXE_yelang-proc-macro-server")
}

/// Return the path to the compiled `test_macro` cdylib fixture.
fn fixture_dylib_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.parent().unwrap().join("target"));
    let file_name = format!(
        "{}test_macro{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    target_dir.join("debug").join(file_name)
}

struct ServerHandle {
    child: Child,
}

impl ServerHandle {
    fn spawn() -> Self {
        let child = Command::new(server_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn server");
        Self { child }
    }

    fn stdin(&mut self) -> &mut std::process::ChildStdin {
        self.child.stdin.as_mut().unwrap()
    }

    fn stdout(&mut self) -> &mut std::process::ChildStdout {
        self.child.stdout.as_mut().unwrap()
    }

    fn handshake(&mut self) {
        write_request(
            self.stdin(),
            &Request::Handshake {
                protocol_version: CURRENT_PROTOCOL_VERSION,
            },
        )
        .unwrap();
        let response = read_response(self.stdout()).unwrap();
        assert!(
            matches!(response, Response::HandshakeAck { .. }),
            "handshake failed: {:?}",
            response
        );
    }

    fn load_library(&mut self, path: impl Into<String>) -> (LibraryHandle, Vec<MacroDescriptor>) {
        write_request(self.stdin(), &Request::LoadLibrary { path: path.into() }).unwrap();
        let response = read_response(self.stdout()).unwrap();
        match response {
            Response::LibraryLoaded { library, macros } => (library, macros),
            other => panic!("expected LibraryLoaded, got {:?}", other),
        }
    }

    fn shutdown(mut self) {
        let _ = write_request(self.stdin(), &Request::Shutdown);
        let _ = self.child.wait();
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = write_request(self.stdin(), &Request::Shutdown);
        let _ = self.child.wait();
    }
}

fn empty_stream() -> WireTokenStream {
    WireTokenStream { trees: Vec::new() }
}

fn single_int_literal(value: &str) -> WireTokenStream {
    WireTokenStream {
        trees: vec![WireTokenTree::Literal {
            text: value.to_string(),
            kind: yelang_proc_macro_bridge::protocol::token::WireLitKind::Int,
            span: yelang_proc_macro_bridge::protocol::token::WireSpan {
                lo: 0,
                hi: 0,
                file: 0,
                syntax_context: 0,
            },
        }],
    }
}

#[test]
fn handshake_succeeds() {
    let mut server = ServerHandle::spawn();
    server.handshake();
}

#[test]
fn load_library_reports_not_found() {
    let mut server = ServerHandle::spawn();
    server.handshake();

    write_request(
        server.stdin(),
        &Request::LoadLibrary {
            path: "/nonexistent/lib.dylib".to_string(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::LibraryNotFound,
                ..
            }
        ),
        "expected LibraryNotFound, got {:?}",
        response
    );
}

#[test]
fn load_fixture_library_returns_descriptors() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (_handle, macros) = server.load_library(dylib.to_string_lossy().to_string());
    assert_eq!(macros.len(), 4);
    assert!(
        macros
            .iter()
            .any(|m| m.name == "make_answer" && m.kind == ProcMacroKind::FunctionLike)
    );
    assert!(
        macros
            .iter()
            .any(|m| m.name == "trace" && m.kind == ProcMacroKind::Attribute)
    );
    assert!(
        macros
            .iter()
            .any(|m| m.name == "answer" && m.kind == ProcMacroKind::Derive)
    );
    assert!(
        macros
            .iter()
            .any(|m| m.name == "panic" && m.kind == ProcMacroKind::FunctionLike)
    );
}

#[test]
fn expand_fn_like_macro() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (handle, _macros) = server.load_library(dylib.to_string_lossy().to_string());

    write_request(
        server.stdin(),
        &Request::ExpandFnLike {
            library: handle,
            macro_index: 0,
            input: empty_stream(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Expanded { output } => {
            assert_eq!(output.trees.len(), 1);
            match &output.trees[0] {
                WireTokenTree::Literal { text, kind, .. } => {
                    assert_eq!(text, "42");
                    assert_eq!(
                        *kind,
                        yelang_proc_macro_bridge::protocol::token::WireLitKind::Int
                    );
                }
                other => panic!("expected integer literal, got {:?}", other),
            }
        }
        other => panic!("expected Expanded, got {:?}", other),
    }
}

#[test]
fn expand_attribute_macro() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (handle, _macros) = server.load_library(dylib.to_string_lossy().to_string());
    let item = single_int_literal("7");

    write_request(
        server.stdin(),
        &Request::ExpandAttr {
            library: handle,
            macro_index: 1,
            args: empty_stream(),
            item: item.clone(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Expanded { output } => {
            assert_eq!(output, item);
        }
        other => panic!("expected Expanded, got {:?}", other),
    }
}

#[test]
fn expand_derive_macro() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (handle, _macros) = server.load_library(dylib.to_string_lossy().to_string());

    write_request(
        server.stdin(),
        &Request::ExpandDerive {
            library: handle,
            macro_index: 2,
            item: empty_stream(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Expanded { output } => {
            assert_eq!(output.trees.len(), 1);
            match &output.trees[0] {
                WireTokenTree::Literal { text, kind, .. } => {
                    assert_eq!(text, "42");
                    assert_eq!(
                        *kind,
                        yelang_proc_macro_bridge::protocol::token::WireLitKind::Int
                    );
                }
                other => panic!("expected integer literal, got {:?}", other),
            }
        }
        other => panic!("expected Expanded, got {:?}", other),
    }
}

#[test]
fn panic_in_macro_returns_panic_response() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (handle, _macros) = server.load_library(dylib.to_string_lossy().to_string());

    write_request(
        server.stdin(),
        &Request::ExpandFnLike {
            library: handle,
            macro_index: 3,
            input: empty_stream(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Panic { message } => {
            assert!(
                message.contains("intentional fixture panic"),
                "got: {}",
                message
            );
        }
        other => panic!("expected Panic, got {:?}", other),
    }
}

#[test]
fn macro_index_out_of_bounds_returns_error() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (handle, _macros) = server.load_library(dylib.to_string_lossy().to_string());

    write_request(
        server.stdin(),
        &Request::ExpandFnLike {
            library: handle,
            macro_index: 99,
            input: empty_stream(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::MacroNotFound,
                ..
            }
        ),
        "expected MacroNotFound, got {:?}",
        response
    );
}

#[test]
fn wrong_macro_kind_returns_error() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (handle, _macros) = server.load_library(dylib.to_string_lossy().to_string());

    // Macro 0 is function-like, but request it as a derive.
    write_request(
        server.stdin(),
        &Request::ExpandDerive {
            library: handle,
            macro_index: 0,
            item: empty_stream(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::MacroNotFound,
                ..
            }
        ),
        "expected MacroNotFound, got {:?}",
        response
    );
}

#[test]
fn unload_library_removes_handle() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut server = ServerHandle::spawn();
    server.handshake();

    let (handle, _macros) = server.load_library(dylib.to_string_lossy().to_string());

    write_request(server.stdin(), &Request::UnloadLibrary { library: handle }).unwrap();
    let response = read_response(server.stdout()).unwrap();
    assert!(
        matches!(response, Response::Expanded { ref output } if output.trees.is_empty()),
        "expected empty Expanded, got {:?}",
        response
    );

    // Expansion after unload should fail.
    write_request(
        server.stdin(),
        &Request::ExpandFnLike {
            library: handle,
            macro_index: 0,
            input: empty_stream(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::LibraryNotFound,
                ..
            }
        ),
        "expected LibraryNotFound after unload, got {:?}",
        response
    );
}
