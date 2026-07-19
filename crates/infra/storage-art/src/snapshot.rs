//! Optional in-memory snapshot encode/decode for `ArtMap`.
//!
//! Snapshots are *not* durable on their own; they are intended for migration,
//! testing, or as an input to a wrapping engine's checkpoint mechanism.
//!
//! Format:
//!
//! ```text
//! Header:
//!   magic:        u32  "ART\0"
//!   version:      u32  1
//!   entry_count:  u64
//!
//! Body: sequence of length-prefixed key/value pairs in sorted order.
//!   key_len:   u32
//!   key:       [u8; key_len]
//!   value_len: u32
//!   value:     [u8; value_len]
//!
//! Trailer:
//!   crc32c over body
//! ```

use bytes::{Buf, Bytes};
use storage_format::crc32c;

use crate::error::{Error, Result};
use crate::map::ArtMap;
use crate::options::ArtMapOptions;

const MAGIC: u32 = 0x41525400; // "ART\0" little-endian
const VERSION: u32 = 1;

/// Encode the current map state into a byte vector.
pub fn encode(map: &ArtMap) -> Result<Vec<u8>> {
    let mut entries = Vec::new();
    map.collect_entries(&mut entries);

    let mut body = Vec::new();
    for (k, v) in &entries {
        body.extend_from_slice(&(k.len() as u32).to_le_bytes());
        body.extend_from_slice(k);
        body.extend_from_slice(&(v.len() as u32).to_le_bytes());
        body.extend_from_slice(v);
    }

    let mut out = Vec::with_capacity(16 + body.len() + 4);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32c(&body).to_le_bytes());
    Ok(out)
}

/// Decode a map state from a byte vector into a new `ArtMap`.
pub fn decode(bytes: &[u8]) -> Result<ArtMap> {
    if bytes.len() < 16 {
        return Err(Error::Corruption("snapshot too short".into()));
    }
    let mut buf = bytes;
    let magic = buf.get_u32_le();
    if magic != MAGIC {
        return Err(Error::Corruption("bad snapshot magic".into()));
    }
    let version = buf.get_u32_le();
    if version != VERSION {
        return Err(Error::Corruption("unsupported snapshot version".into()));
    }
    let entry_count = buf.get_u64_le() as usize;

    if buf.len() < 4 {
        return Err(Error::Corruption("snapshot missing crc".into()));
    }
    let body_len = buf.len() - 4;
    let body = &buf[..body_len];
    let stored_crc = u32::from_le_bytes([buf[body_len], buf[body_len + 1], buf[body_len + 2], buf[body_len + 3]]);
    if crc32c(body) != stored_crc {
        return Err(Error::Corruption("snapshot crc mismatch".into()));
    }

    let mut body = body;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        if body.len() < 4 {
            return Err(Error::Corruption("truncated key length".into()));
        }
        let key_len = body.get_u32_le() as usize;
        if body.len() < key_len + 4 {
            return Err(Error::Corruption("truncated key".into()));
        }
        let key = &body[..key_len];
        body.advance(key_len);
        let value_len = body.get_u32_le() as usize;
        if body.len() < value_len {
            return Err(Error::Corruption("truncated value".into()));
        }
        let value = &body[..value_len];
        body.advance(value_len);
        entries.push((Bytes::copy_from_slice(key), Bytes::copy_from_slice(value)));
    }

    let map = ArtMap::new(ArtMapOptions::default());
    for (k, v) in entries {
        map.insert(&k, &v)?;
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_snapshot() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"a", b"1").unwrap();
        map.insert(b"b", b"2").unwrap();
        let bytes = encode(&map).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.get(b"a"), Some(Bytes::from_static(b"1")));
        assert_eq!(decoded.get(b"b"), Some(Bytes::from_static(b"2")));
    }

    #[test]
    fn empty_snapshot() {
        let map = ArtMap::new(ArtMapOptions::default());
        let bytes = encode(&map).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert!(decoded.is_empty());
    }
}
