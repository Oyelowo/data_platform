//! Tests for proc-macro client crash detection and restart.

mod macro_fixture;

use macro_fixture::{fixture_dylib_path, server_path};
use yelang_macro::proc_macro::{ProcMacroClient, ProcMacroClientError};
use yelang_proc_macro_bridge::sandbox::Limits;

fn empty_stream() -> yelang_proc_macro_bridge::protocol::WireTokenStream {
    yelang_proc_macro_bridge::protocol::WireTokenStream { trees: Vec::new() }
}

fn default_call_site() -> yelang_proc_macro_bridge::protocol::token::WireSpan {
    yelang_proc_macro_bridge::protocol::token::WireSpan {
        lo: 0,
        hi: 0,
        file: 0,
        syntax_context: 0,
    }
}

#[test]
fn client_detects_server_death_and_restarts() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let library = client
        .load_library(&dylib.to_string_lossy())
        .expect("load fixture library");

    // `exit_macro` (index 8) kills the server process from inside the macro.
    let err = client
        .expand_fn_like(
            library.handle,
            8,
            empty_stream(),
            default_call_site(),
            Limits::default(),
        )
        .expect_err("expected server death");
    assert!(
        matches!(err, ProcMacroClientError::ServerDied),
        "expected ServerDied, got {:?}",
        err
    );

    // Restart the server and prove it is usable again.
    client.restart().expect("restart server");
    let library = client
        .load_library(&dylib.to_string_lossy())
        .expect("reload fixture library after restart");
    let (output, _) = client
        .expand_fn_like(
            library.handle,
            0,
            empty_stream(),
            default_call_site(),
            Limits::default(),
        )
        .expect("expand after restart");
    assert_eq!(output.trees.len(), 1);
}

#[test]
fn client_restarts_dead_server_transparently_between_requests() {
    let dylib = fixture_dylib_path();
    if !dylib.exists() {
        eprintln!("fixture dylib not found at {}; skipping", dylib.display());
        return;
    }

    let mut client = ProcMacroClient::spawn(server_path()).expect("spawn server");
    let library = client
        .load_library(&dylib.to_string_lossy())
        .expect("load fixture library");

    // Kill the server.
    let _ = client.expand_fn_like(
        library.handle,
        8,
        empty_stream(),
        default_call_site(),
        Limits::default(),
    );

    // `ensure_alive` should detect the dead process and start a fresh one.
    client.ensure_alive().expect("ensure_alive should restart");
    assert!(
        client.is_alive(),
        "server should be alive after ensure_alive"
    );
}
