use std::fs::OpenOptions;

use storage_file::{read_exact_at, write_all_at};
use tempfile::TempDir;

#[test]
fn write_and_read_at_offset() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("data");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
        .unwrap();

    write_all_at(&mut file, 16, b"hello").unwrap();

    let mut buf = [0u8; 5];
    read_exact_at(&file, 16, &mut buf).unwrap();
    assert_eq!(&buf, b"hello");
}
