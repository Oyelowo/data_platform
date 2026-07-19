use std::fs;

use storage_file::atomic_write;
use tempfile::TempDir;

#[test]
fn atomic_write_creates_file_with_content() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("meta");
    atomic_write(&dest, b"hello world").unwrap();
    assert!(dest.exists());
    assert_eq!(fs::read(&dest).unwrap(), b"hello world");
}

#[test]
fn atomic_write_replaces_existing_file() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("meta");
    fs::write(&dest, b"old").unwrap();
    atomic_write(&dest, b"new").unwrap();
    assert_eq!(fs::read(&dest).unwrap(), b"new");
}

#[test]
fn atomic_write_does_not_leave_tmp_on_success() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("meta");
    atomic_write(&dest, b"data").unwrap();
    assert!(!dir.path().join("meta.tmp").exists());
}
