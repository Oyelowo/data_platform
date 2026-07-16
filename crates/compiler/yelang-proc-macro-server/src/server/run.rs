//! Main server request loop.

use std::path::Path;

use yelang_proc_macro_bridge::{
    ErrorCode,
    protocol::{
        CURRENT_PROTOCOL_VERSION, LibraryHandle, Request, Response, WireTokenStream,
        negotiate_version, token::WireDiagnostic,
    },
};

use super::{library::LoadedLibrary, session::Session};
use crate::executor::{InvokeError, invoke_attr, invoke_derive, invoke_fn_like};

/// Run the server until shutdown.
pub fn run() {
    let mut session = Session::new();

    loop {
        let request = match crate::protocol::read_request_from_stdin() {
            Ok(r) => r,
            Err(e) => {
                let _ = send_error(ErrorCode::Internal, e.to_string());
                continue;
            }
        };

        if matches!(request, Request::Shutdown) {
            break;
        }

        let response = handle_request(&mut session, request);
        let _ = crate::protocol::write_response_to_stdout(&response);
    }
}

fn handle_request(session: &mut Session, request: Request) -> Response {
    match request {
        Request::Handshake { protocol_version } => {
            match negotiate_version(protocol_version, CURRENT_PROTOCOL_VERSION) {
                Ok(v) => Response::HandshakeAck {
                    protocol_version: v,
                },
                Err(_) => error(ErrorCode::ProtocolMismatch, "protocol version mismatch"),
            }
        }
        Request::LoadLibrary { path } => {
            if !Path::new(&path).exists() {
                return error(
                    ErrorCode::LibraryNotFound,
                    format!("library not found: {path}"),
                );
            }
            match LoadedLibrary::load(&path) {
                Ok(lib) => {
                    let descriptors = lib.descriptors.clone();
                    let handle = session.insert_library(lib);
                    Response::LibraryLoaded {
                        library: handle,
                        macros: descriptors,
                    }
                }
                Err(e) => error(ErrorCode::LibraryLoadFailed, e.to_string()),
            }
        }
        Request::UnloadLibrary { library } => {
            session.remove_library(library);
            Response::Expanded {
                output: WireTokenStream { trees: Vec::new() },
            }
        }
        Request::Shutdown => unreachable!("handled above"),
        Request::ExpandFnLike {
            library,
            macro_index,
            input,
        } => expand(session, library, macro_index, |lib| {
            invoke_fn_like(lib, macro_index, input)
        }),
        Request::ExpandAttr {
            library,
            macro_index,
            args,
            item,
        } => expand(session, library, macro_index, |lib| {
            invoke_attr(lib, macro_index, args, item)
        }),
        Request::ExpandDerive {
            library,
            macro_index,
            item,
        } => expand(session, library, macro_index, |lib| {
            invoke_derive(lib, macro_index, item)
        }),
    }
}

fn expand<F>(session: &Session, library: LibraryHandle, _macro_index: u32, f: F) -> Response
where
    F: FnOnce(&LoadedLibrary) -> Result<(WireTokenStream, Vec<WireDiagnostic>), InvokeError>,
{
    match session.get_library(library) {
        Some(lib) => match f(lib) {
            Ok((output, diagnostics)) => {
                for d in diagnostics {
                    let _ = crate::protocol::write_response_to_stdout(&Response::Diagnostic {
                        diagnostic: d,
                    });
                }
                Response::Expanded { output }
            }
            Err(InvokeError::Panic(message)) => Response::Panic { message },
            Err(e) => error(error_code_for_invoke_error(&e), e.to_string()),
        },
        None => error(ErrorCode::LibraryNotFound, "library handle invalid"),
    }
}

fn error_code_for_invoke_error(e: &InvokeError) -> ErrorCode {
    match e {
        InvokeError::MacroIndexOutOfBounds | InvokeError::WrongKind { .. } => {
            ErrorCode::MacroNotFound
        }
        InvokeError::InputSerialization(_)
        | InvokeError::OutputDeserialization(_)
        | InvokeError::NullOutput => ErrorCode::InvalidInput,
        InvokeError::Internal(_) => ErrorCode::Internal,
        InvokeError::Panic(_) => unreachable!("panic mapped separately"),
    }
}

fn error(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error {
        code,
        message: message.into(),
    }
}

fn send_error(
    code: ErrorCode,
    message: String,
) -> Result<(), yelang_proc_macro_bridge::protocol::SerializeError> {
    crate::protocol::write_response_to_stdout(&error(code, message))
}
