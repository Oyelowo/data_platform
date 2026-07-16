/*!
 * Wire protocol for compiler <-> proc-macro server communication.
 */

pub mod message;
pub mod serialize;
pub mod token;
pub mod version;

pub use message::{ErrorCode, LibraryHandle, MacroDescriptor, ProcMacroKind, Request, Response};
pub use serialize::{
    MessageReader, MessageWriter, SerializeError, read_frame, read_request, read_response,
    write_frame, write_request, write_response,
};
pub use token::WireTokenStream;
pub use version::{CURRENT_PROTOCOL_VERSION, negotiate_version};
