//! Atomic filesystem writes shared by file built-ins and checkpoints.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static TEMPORARY_FILE_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let destination = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => match fs::canonicalize(path) {
            Ok(destination) => destination,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let target = fs::read_link(path)?;
                let target = if target.is_absolute() {
                    target
                } else {
                    path.parent().unwrap_or(Path::new(".")).join(target)
                };
                let file_name = target.file_name().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "symlink target has no file name",
                    )
                })?;
                fs::canonicalize(target.parent().unwrap_or(Path::new(".")))?.join(file_name)
            }
            Err(error) => return Err(error),
        },
        Ok(_) => path.to_path_buf(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error),
    };
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = destination.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no file name",
        )
    })?;
    let existing_permissions = match fs::metadata(&destination) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let (temporary, mut file) = loop {
        let id = TEMPORARY_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = file_name.to_os_string();
        temporary_name.push(format!(".simply-tmp-{}-{id}", std::process::id()));
        let temporary = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        if let Some(permissions) = existing_permissions {
            file.set_permissions(permissions)?;
        }
        drop(file);
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
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
}
