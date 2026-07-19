use storage_format::{crc32c, Crc32c};

#[test]
fn crc32c_is_consistent() {
    let data = b"hello world";
    let a = crc32c(data);
    let mut acc = Crc32c::new();
    acc.update(data);
    assert_eq!(a, acc.finalize());
}

#[test]
fn crc32c_detects_corruption() {
    let data = b"hello world";
    let mut corrupted = data.to_vec();
    corrupted[0] ^= 0x01;
    assert_ne!(crc32c(data), crc32c(&corrupted));
}
