//! Integration tests for the proc-macro server.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use yelang_proc_macro_bridge::protocol::{
    CURRENT_PROTOCOL_VERSION, ErrorCode, Request, Response,
    serialize::{read_response, write_request},
};

fn server_path() -> &'static str {
    env!("CARGO_BIN_EXE_yelang-proc-macro-server")
}

#[test]
fn handshake_succeeds() {
    let mut child = Command::new(server_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn server");

    let stdin = child.stdin.as_mut().unwrap();
    let stdout = child.stdout.as_mut().unwrap();

    write_request(
        stdin,
        &Request::Handshake {
            protocol_version: CURRENT_PROTOCOL_VERSION,
        },
    )
    .unwrap();

    let response = read_response(stdout).unwrap();
    assert!(matches!(response, Response::HandshakeAck { .. }));

    write_request(stdin, &Request::Shutdown).unwrap();
    let _ = child.wait();
}

#[test]
fn load_library_reports_not_found() {
    let mut child = Command::new(server_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn server");

    let stdin = child.stdin.as_mut().unwrap();
    let stdout = child.stdout.as_mut().unwrap();

    write_request(
        stdin,
        &Request::Handshake {
            protocol_version: CURRENT_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let _ = read_response(stdout).unwrap();

    write_request(
        stdin,
        &Request::LoadLibrary {
            path: "/nonexistent/lib.dylib".to_string(),
        },
    )
    .unwrap();

    let response = read_response(stdout).unwrap();
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

    write_request(stdin, &Request::Shutdown).unwrap();
    let _ = child.wait();
}
