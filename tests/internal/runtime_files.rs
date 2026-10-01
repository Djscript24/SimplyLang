// Internal unit tests for src/runtime/files.rs.
use super::atomic_write;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static TEST_ID: AtomicU64 = AtomicU64::new(0);

fn test_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "simply-atomic-write-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn replaces_existing_file_contents() {
    let path = test_path();
    fs::write(&path, "old").expect("failed to create test destination");
    atomic_write(&path, b"new").expect("atomic replacement failed");
    assert_eq!(
        fs::read_to_string(&path).expect("failed to read destination"),
        "new"
    );
    fs::remove_file(path).expect("failed to remove test destination");
}

#[test]
fn failed_replacement_preserves_existing_file() {
    let directory = test_path();
    fs::create_dir_all(&directory).expect("failed to create test directory");
    let destination = directory.join("destination");
    fs::write(&destination, "keep").expect("failed to create test destination");
    let source_directory = directory.join("source");
    fs::create_dir_all(&source_directory).expect("failed to create source directory");

    assert!(atomic_write(&source_directory, b"replacement").is_err());
    assert_eq!(
        fs::read_to_string(&destination).expect("failed to read destination"),
        "keep"
    );
    fs::remove_dir_all(directory).expect("failed to remove test directory");
}

#[cfg(unix)]
#[test]
fn replacement_preserves_permissions_and_follows_existing_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let path = test_path();
    let target = path.with_extension("target");
    fs::write(&target, "old").expect("failed to create symlink target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
        .expect("failed to set target permissions");
    symlink(&target, &path).expect("failed to create test symlink");

    atomic_write(&path, b"new").expect("atomic symlink write failed");

    assert_eq!(
        fs::read_to_string(&target).expect("failed to read target"),
        "new"
    );
    assert!(
        fs::symlink_metadata(&path)
            .expect("failed to inspect symlink")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::metadata(&target)
            .expect("failed to inspect target")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fs::remove_file(path).expect("failed to remove test symlink");
    fs::remove_file(target).expect("failed to remove test target");
}

#[cfg(unix)]
#[test]
fn replacement_follows_dangling_symlinks_when_target_parent_exists() {
    use std::os::unix::fs::symlink;

    let path = test_path();
    let target = path.with_extension("target");
    symlink(&target, &path).expect("failed to create dangling symlink");

    atomic_write(&path, b"created").expect("atomic dangling-symlink write failed");

    assert_eq!(
        fs::read_to_string(&target).expect("failed to read created target"),
        "created"
    );
    assert!(
        fs::symlink_metadata(&path)
            .expect("failed to inspect symlink")
            .file_type()
            .is_symlink()
    );
    fs::remove_file(path).expect("failed to remove test symlink");
    fs::remove_file(target).expect("failed to remove created target");
}
