//! Tests for the proc-macro bridge protocol.

use yelang_proc_macro_bridge::protocol::{
    CURRENT_PROTOCOL_VERSION, ErrorCode, LibraryHandle, MacroDescriptor, ProcMacroKind, Request,
    Response, WireTokenStream, negotiate_version,
    serialize::{read_request, write_request},
    token::{WireDelimiter, WireHygienePayload, WireSpan, WireTokenTree},
};
use yelang_proc_macro_bridge::sandbox::Limits;

#[test]
fn version_negotiation_chooses_minimum() {
    assert_eq!(
        negotiate_version(CURRENT_PROTOCOL_VERSION, CURRENT_PROTOCOL_VERSION).unwrap(),
        CURRENT_PROTOCOL_VERSION
    );
    assert_eq!(negotiate_version(1, 2).unwrap(), 1);
    assert_eq!(negotiate_version(2, 1).unwrap(), 1);
}

#[test]
fn version_negotiation_rejects_zero() {
    assert!(negotiate_version(0, 1).is_err());
}

#[test]
fn request_round_trip() {
    let request = Request::Handshake {
        protocol_version: CURRENT_PROTOCOL_VERSION,
    };
    let mut buf = Vec::new();
    write_request(&mut buf, &request).unwrap();
    let decoded = read_request(&mut buf.as_slice()).unwrap();
    assert_eq!(request, decoded);
}

#[test]
fn expand_fn_like_round_trip() {
    let request = Request::ExpandFnLike {
        library: LibraryHandle::default(),
        macro_index: 0,
        input: WireTokenStream {
            trees: vec![WireTokenTree::Ident {
                text: "foo".to_string(),
                span: WireSpan {
                    lo: 0,
                    hi: 3,
                    file: 0,
                    syntax_context: 0,
                },
                is_raw: false,
            }],
        },
        call_site: WireSpan {
            lo: 10,
            hi: 20,
            file: 1,
            syntax_context: 0,
        },
        def_site: WireSpan {
            lo: 100,
            hi: 110,
            file: 2,
            syntax_context: 0,
        },
        hygiene: WireHygienePayload::empty(),
        limits: Limits::default(),
    };
    let mut buf = Vec::new();
    write_request(&mut buf, &request).unwrap();
    let decoded = read_request(&mut buf.as_slice()).unwrap();
    assert_eq!(request, decoded);
}

#[test]
fn response_round_trip() {
    use yelang_proc_macro_bridge::protocol::serialize::{read_response, write_response};

    let response = Response::LibraryLoaded {
        library: LibraryHandle::default(),
        macros: vec![MacroDescriptor {
            name: "MyDerive".to_string(),
            kind: ProcMacroKind::Derive,
        }],
    };
    let mut buf = Vec::new();
    write_response(&mut buf, &response).unwrap();
    let decoded = read_response(&mut buf.as_slice()).unwrap();
    assert_eq!(response, decoded);
}

#[test]
fn error_response_round_trip() {
    use yelang_proc_macro_bridge::protocol::serialize::{read_response, write_response};

    let response = Response::Error {
        code: ErrorCode::LibraryNotFound,
        message: "no such library".to_string(),
    };
    let mut buf = Vec::new();
    write_response(&mut buf, &response).unwrap();
    let decoded = read_response(&mut buf.as_slice()).unwrap();
    assert_eq!(response, decoded);
}
