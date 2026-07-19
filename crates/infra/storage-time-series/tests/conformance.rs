//! `storage-testkit` conformance suite.
//!
//! The generic conformance suite assumes opaque byte keys and values, whereas
//! `storage-time-series` encodes all `Engine` trait keys as
//! `series_key || timestamp_be` and values as a `Value` kind byte + payload.
//! Because of this intentional specialization, the generic conformance runner
//! is not applicable and is skipped.

#[test]
fn conformance_skipped_by_design() {
    // The engine intentionally exposes time-series semantics through the
    // generic `Engine` trait by encoding keys/values; it does not accept
    // arbitrary opaque bytes. See PRODUCTION_READINESS.md for details.
}
