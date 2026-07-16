//! Framed I/O over stdin/stdout.

use std::io;

pub use yelang_proc_macro_bridge::protocol::{
    Request, Response, SerializeError, read_request, write_response,
};

pub fn read_request_from_stdin() -> Result<Request, SerializeError> {
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    read_request(&mut handle)
}

pub fn write_response_to_stdout(response: &Response) -> Result<(), SerializeError> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    write_response(&mut handle, response)
}
