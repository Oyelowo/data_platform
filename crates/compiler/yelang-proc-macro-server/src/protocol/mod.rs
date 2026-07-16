/*!
 * Framed I/O over stdin/stdout.
 */

pub mod io;

pub use io::{
    Request, Response, SerializeError, read_request, read_request_from_stdin, write_response,
    write_response_to_stdout,
};
