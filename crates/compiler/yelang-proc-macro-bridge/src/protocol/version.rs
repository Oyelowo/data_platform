//! Protocol version negotiation.

/// Current wire protocol version.
pub const CURRENT_PROTOCOL_VERSION: u32 = 1;

/// Negotiate the highest shared protocol version.
pub fn negotiate_version(client: u32, server: u32) -> Result<u32, NegotiationError> {
    let chosen = client.min(server);
    if chosen == 0 {
        Err(NegotiationError::NoSharedVersion)
    } else {
        Ok(chosen)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NegotiationError {
    #[error("no shared proc-macro protocol version")]
    NoSharedVersion,
}
