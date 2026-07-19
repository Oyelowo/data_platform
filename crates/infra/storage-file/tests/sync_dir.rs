use std::fs;

use storage_file::sync_dir;
use tempfile::TempDir;

#[test]
fn sync_dir_does_not_fail_on_empty_directory() {
    let dir = TempDir::new().unwrap();
    sync_dir(dir.path()).unwrap();
}

#[test]
fn sync_dir_makes_new_file_durable() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("file"), b"x").unwrap();
    sync_dir(dir.path()).unwrap();
}
