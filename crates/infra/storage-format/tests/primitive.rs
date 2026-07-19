use storage_format::{read_u16_le, read_u32_le, read_u64_le, write_u16_le, write_u32_le, write_u64_le};

#[test]
fn primitive_roundtrip() {
    let mut buf = [0u8; 8];

    write_u16_le(&mut buf, 0x1234);
    assert_eq!(read_u16_le(&buf), 0x1234);

    write_u32_le(&mut buf, 0xDEAD_BEEF);
    assert_eq!(read_u32_le(&buf), 0xDEAD_BEEF);

    write_u64_le(&mut buf, 0x1234_5678_9ABC_DEF0);
    assert_eq!(read_u64_le(&buf), 0x1234_5678_9ABC_DEF0);
}
