//! Client that talks to the proc-macro server.

use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use thiserror::Error;
use yelang_proc_macro_bridge::protocol::token::{WireDiagnostic, WireHygienePayload, WireSpan};
use yelang_proc_macro_bridge::protocol::{
    CURRENT_PROTOCOL_VERSION, ErrorCode, LibraryHandle, MacroDescriptor, Request, Response,
    WireTokenStream, read_response, write_request,
};
use yelang_proc_macro_bridge::sandbox::Limits;

#[derive(Debug, Error, Clone)]
pub enum ProcMacroClientError {
    #[error("failed to spawn server: {0}")]
    Spawn(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("server error {0:?}: {1}")]
    Server(ErrorCode, String),
    #[error("server panicked: {0}")]
    Panic(String),
    #[error("loaded library does not match its manifest: {0}")]
    Validation(String),
    #[error("proc-macro server process died")]
    ServerDied,
}

impl From<std::io::Error> for ProcMacroClientError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// A library loaded on the server, with the descriptors of every macro it
/// exports (position in `descriptors` is the `macro_index` used in expansion
/// requests).
#[derive(Debug, Clone)]
pub struct LoadedLibrary {
    pub handle: LibraryHandle,
    pub descriptors: Vec<MacroDescriptor>,
}

/// Client connection to a proc-macro server.
pub struct ProcMacroClient {
    server_path: String,
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl ProcMacroClient {
    /// Spawn the server binary and perform the handshake.
    pub fn spawn(server_path: &str) -> Result<Self, ProcMacroClientError> {
        let mut child = Command::new(server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ProcMacroClientError::Spawn(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProcMacroClientError::Spawn("no stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProcMacroClientError::Spawn("no stdout".to_string()))?;

        let mut client = Self {
            server_path: server_path.to_string(),
            child,
            stdin,
            stdout,
        };

        client.handshake()?;
        Ok(client)
    }

    /// Spawn the server located by the default lookup rules:
    ///
    /// 1. the `YELANG_PROC_MACRO_SERVER` environment variable, then
    /// 2. `yelang-proc-macro-server` (with `.exe` on Windows) next to the
    ///    current executable.
    pub fn spawn_default() -> Result<Self, ProcMacroClientError> {
        if let Ok(path) = std::env::var("YELANG_PROC_MACRO_SERVER")
            && !path.trim().is_empty()
        {
            return Self::spawn(&path);
        }
        let sibling = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
            .map(|dir| dir.join(server_binary_name()));
        match sibling {
            Some(path) if path.is_file() => Self::spawn(&path.to_string_lossy()),
            _ => Err(ProcMacroClientError::Spawn(format!(
                "proc-macro server not found: set YELANG_PROC_MACRO_SERVER or place \
                 `{server}` next to the compiler executable",
                server = server_binary_name(),
            ))),
        }
    }

    fn handshake(&mut self) -> Result<(), ProcMacroClientError> {
        self.send_request(&Request::Handshake {
            protocol_version: CURRENT_PROTOCOL_VERSION,
        })?;
        match self.read_response()? {
            Response::HandshakeAck { .. } => Ok(()),
            Response::Error { code, message } => Err(ProcMacroClientError::Server(code, message)),
            other => Err(ProcMacroClientError::Protocol(format!(
                "unexpected handshake response: {:?}",
                other
            ))),
        }
    }

    /// Returns `true` if the server process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Kill the current server process (if any) and spawn a fresh one.
    ///
    /// Any library handles previously returned by this connection are invalid
    /// after a restart; callers must reload the libraries they intend to use.
    pub fn restart(&mut self) -> Result<(), ProcMacroClientError> {
        let _ = self.child.kill();
        let _ = self.child.wait();

        let mut child = Command::new(&self.server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ProcMacroClientError::Spawn(e.to_string()))?;

        self.stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProcMacroClientError::Spawn("no stdin".to_string()))?;
        self.stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProcMacroClientError::Spawn("no stdout".to_string()))?;
        self.child = child;

        self.handshake()
    }

    /// Ensure the server is alive, restarting it if it has exited.
    pub fn ensure_alive(&mut self) -> Result<(), ProcMacroClientError> {
        if self.is_alive() {
            Ok(())
        } else {
            self.restart()
        }
    }

    /// Load a proc-macro dynamic library by path, returning its handle and
    /// the descriptors of every macro it exports.
    pub fn load_library(&mut self, path: &str) -> Result<LoadedLibrary, ProcMacroClientError> {
        self.ensure_alive()?;
        self.send_request(&Request::LoadLibrary {
            path: path.to_string(),
        })?;
        match self.read_response()? {
            Response::LibraryLoaded { library, macros } => Ok(LoadedLibrary {
                handle: library,
                descriptors: macros,
            }),
            Response::Error { code, message } => Err(ProcMacroClientError::Server(code, message)),
            other => Err(ProcMacroClientError::Protocol(format!(
                "unexpected load response: {:?}",
                other
            ))),
        }
    }

    /// Invoke a function-like macro.
    #[allow(clippy::too_many_arguments)]
    pub fn expand_fn_like(
        &mut self,
        library: LibraryHandle,
        macro_index: u32,
        input: WireTokenStream,
        call_site: WireSpan,
        def_site: WireSpan,
        hygiene: WireHygienePayload,
        limits: Limits,
    ) -> Result<(WireTokenStream, Vec<WireDiagnostic>, WireHygienePayload), ProcMacroClientError>
    {
        self.ensure_alive()?;
        self.send_request(&Request::ExpandFnLike {
            library,
            macro_index,
            input,
            call_site,
            def_site,
            hygiene,
            limits,
        })?;
        self.read_expanded()
    }

    /// Invoke an attribute macro.
    #[allow(clippy::too_many_arguments)]
    pub fn expand_attr(
        &mut self,
        library: LibraryHandle,
        macro_index: u32,
        args: WireTokenStream,
        item: WireTokenStream,
        call_site: WireSpan,
        def_site: WireSpan,
        hygiene: WireHygienePayload,
        limits: Limits,
    ) -> Result<(WireTokenStream, Vec<WireDiagnostic>, WireHygienePayload), ProcMacroClientError>
    {
        self.ensure_alive()?;
        self.send_request(&Request::ExpandAttr {
            library,
            macro_index,
            args,
            item,
            call_site,
            def_site,
            hygiene,
            limits,
        })?;
        self.read_expanded()
    }

    /// Invoke a derive macro.
    #[allow(clippy::too_many_arguments)]
    pub fn expand_derive(
        &mut self,
        library: LibraryHandle,
        macro_index: u32,
        item: WireTokenStream,
        call_site: WireSpan,
        def_site: WireSpan,
        hygiene: WireHygienePayload,
        limits: Limits,
    ) -> Result<(WireTokenStream, Vec<WireDiagnostic>, WireHygienePayload), ProcMacroClientError>
    {
        self.ensure_alive()?;
        self.send_request(&Request::ExpandDerive {
            library,
            macro_index,
            item,
            call_site,
            def_site,
            hygiene,
            limits,
        })?;
        self.read_expanded()
    }

    fn read_expanded(
        &mut self,
    ) -> Result<(WireTokenStream, Vec<WireDiagnostic>, WireHygienePayload), ProcMacroClientError>
    {
        let mut diagnostics = Vec::new();
        loop {
            match self.read_response()? {
                Response::Expanded { output, hygiene } => {
                    return Ok((output, diagnostics, hygiene));
                }
                Response::Diagnostic { diagnostic } => diagnostics.push(diagnostic),
                Response::Panic { message } => return Err(ProcMacroClientError::Panic(message)),
                Response::Error { code, message } => {
                    return Err(ProcMacroClientError::Server(code, message));
                }
                other => {
                    return Err(ProcMacroClientError::Protocol(format!(
                        "unexpected expansion response: {:?}",
                        other
                    )));
                }
            }
        }
    }

    fn send_request(&mut self, request: &Request) -> Result<(), ProcMacroClientError> {
        write_request(&mut self.stdin, request).map_err(|e| self.map_comm_error(e))
    }

    fn read_response(&mut self) -> Result<Response, ProcMacroClientError> {
        read_response(&mut self.stdout).map_err(|e| self.map_comm_error(e))
    }

    fn map_comm_error(
        &mut self,
        e: yelang_proc_macro_bridge::protocol::SerializeError,
    ) -> ProcMacroClientError {
        // A broken pipe or unexpected EOF almost always means the server
        // process died. Report that explicitly so the runtime can reconnect.
        // Parse errors and oversized frames are kept as protocol errors.
        if matches!(e, yelang_proc_macro_bridge::protocol::SerializeError::Io(_))
            || !self.is_alive()
        {
            ProcMacroClientError::ServerDied
        } else {
            ProcMacroClientError::Protocol(e.to_string())
        }
    }
}

impl Drop for ProcMacroClient {
    fn drop(&mut self) {
        let _ = self.send_request(&Request::Shutdown);
        let _ = self.child.wait();
    }
}

fn server_binary_name() -> &'static str {
    if cfg!(windows) {
        "yelang-proc-macro-server.exe"
    } else {
        "yelang-proc-macro-server"
    }
}
