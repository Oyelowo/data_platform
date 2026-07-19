use proptest::prelude::*;
use storage_format::{decode_uvarint, encode_uvarint, encoded_uvarint_len, read_uvarint, write_uvarint};

#[test]
fn uvarint_roundtrip() {
    for value in [0u64, 1, 127, 128, 255, 256, u64::MAX] {
        let mut buf = [0u8; 10];
        let len = encode_uvarint(&mut buf, value);
        assert_eq!(len, encoded_uvarint_len(value));
        let (decoded, consumed) = decode_uvarint(&buf[..len]).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(consumed, len);
    }
}

#[test]
fn uvarint_io_roundtrip() {
    let mut buf = Vec::new();
    for value in [0u64, 1, 127, 128, 16384, u64::MAX] {
        buf.clear();
        write_uvarint(&mut buf, value).unwrap();
        let decoded = read_uvarint(&mut &buf[..]).unwrap();
        assert_eq!(decoded, value);
    }
}

proptest! {
    #[test]
    fn uvarint_all_values(value: u64) {
        let mut buf = [0u8; 10];
        let len = encode_uvarint(&mut buf, value);
        let (decoded, consumed) = decode_uvarint(&buf[..len]).unwrap();
        prop_assert_eq!(decoded, value);
        prop_assert_eq!(consumed, len);
    }
}
