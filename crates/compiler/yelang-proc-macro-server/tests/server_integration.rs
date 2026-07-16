//! Integration tests for the proc-macro server.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use yelang_macro::proc_macro::HOST_TRIPLE;
use yelang_proc_macro_bridge::protocol::{
    CURRENT_PROTOCOL_VERSION, ErrorCode, LibraryHandle, MacroDescriptor, ProcMacroKind, Request,
    Response, WireTokenStream,
    serialize::{read_response, write_request},
    token::{WireDiagnostic, WireHygienePayload, WireLevel, WireSpan, WireTokenTree},
};
use yelang_proc_macro_bridge::sandbox::Limits;

fn server_path() -> &'static str {
    env!("CARGO_BIN_EXE_yelang-proc-macro-server")
}

fn default_call_site() -> WireSpan {
    WireSpan {
        lo: 0,
        hi: 0,
        file: 0,
        syntax_context: 0,
    }
}

fn default_def_site() -> WireSpan {
    WireSpan {
        lo: 0,
        hi: 0,
        file: 0,
        syntax_context: 0,
    }
}

fn empty_hygiene() -> WireHygienePayload {
    WireHygienePayload::empty()
}

/// Return the path to the compiled `test_macro` dylib fixture.
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
    // Cargo may place host artifacts directly under `debug` or under a
    // configured target triple directory; accept either.
    [
        target_dir.join("debug").join(&file_name),
        target_dir.join(HOST_TRIPLE).join("debug").join(&file_name),
    ]
    .into_iter()
    .find(|p| p.exists())
    .unwrap_or_else(|| target_dir.join("debug").join(file_name))
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

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
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
                // File and syntax-context IDs are 1-based in this codebase;
                // 0 would panic during deserialization inside the proc macro.
                file: 1,
                syntax_context: 1,
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
    assert_eq!(macros.len(), 9);
    for (name, kind) in [
        ("make_answer", ProcMacroKind::FunctionLike),
        ("trace", ProcMacroKind::Attribute),
        ("answer", ProcMacroKind::Derive),
        ("generate_const", ProcMacroKind::Derive),
        ("emit_warning", ProcMacroKind::FunctionLike),
        ("explode", ProcMacroKind::FunctionLike),
        ("slow_macro", ProcMacroKind::FunctionLike),
        ("huge_macro", ProcMacroKind::FunctionLike),
        ("exit_macro", ProcMacroKind::FunctionLike),
    ] {
        assert!(
            macros.iter().any(|m| m.name == name && m.kind == kind),
            "missing macro {name:?} of kind {kind:?}"
        );
    }
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
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Expanded { output, .. } => {
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
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Expanded { output, .. } => {
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
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Expanded { output, .. } => {
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
fn panic_in_macro_returns_error_diagnostic() {
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
            macro_index: 5,
            input: empty_stream(),
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Diagnostic {
            diagnostic:
                WireDiagnostic {
                    level: WireLevel::Error,
                    message,
                    ..
                },
        } => {
            assert!(
                message.contains("macro panicked") && message.contains("intentional fixture panic"),
                "got: {}",
                message
            );
        }
        other => panic!("expected Error diagnostic, got {:?}", other),
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
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
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
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
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
fn expand_generate_const_derive_macro() {
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
            macro_index: 3,
            item: empty_stream(),
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    match response {
        Response::Expanded { output, .. } => {
            assert!(!output.trees.is_empty());
        }
        other => panic!("expected Expanded, got {:?}", other),
    }
}

#[test]
fn expand_emit_warning_macro_returns_diagnostic() {
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
            macro_index: 4,
            input: empty_stream(),
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
        },
    )
    .unwrap();

    let mut got_diagnostic = false;
    loop {
        let response = read_response(server.stdout()).unwrap();
        match response {
            Response::Expanded { .. } => break,
            Response::Diagnostic { diagnostic } => {
                assert_eq!(diagnostic.message, "intentional fixture warning");
                got_diagnostic = true;
            }
            other => panic!("expected Diagnostic or Expanded, got {:?}", other),
        }
    }
    assert!(got_diagnostic);
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
        matches!(response, Response::Expanded { ref output, .. } if output.trees.is_empty()),
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
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits::default(),
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

#[test]
fn fn_like_macro_times_out() {
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
            macro_index: 6,
            input: empty_stream(),
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits {
                max_cpu_seconds: 1,
                ..Limits::default()
            },
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::ExpansionTimeout,
                ..
            }
        ),
        "expected ExpansionTimeout, got {:?}",
        response
    );

    // The server exits after a timeout so the runaway worker cannot starve
    // future requests. Give it a moment and confirm the process is gone.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if !server.is_alive() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("server process survived a timeout");
}

#[test]
fn fn_like_macro_output_limit() {
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
            macro_index: 7,
            input: empty_stream(),
            call_site: default_call_site(),
            def_site: default_def_site(),
            hygiene: empty_hygiene(),
            limits: Limits {
                max_output_tokens: 100,
                ..Limits::default()
            },
        },
    )
    .unwrap();

    let response = read_response(server.stdout()).unwrap();
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::ExpansionTooLarge,
                ..
            }
        ),
        "expected ExpansionTooLarge, got {:?}",
        response
    );
}
