//! Small file system helpers: atomic writes and link-safe recursive removal.

use std::ffi::OsString;
use std::fs::{self, File, Metadata};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Whether the entry is a link (symlink, junction or any other reparse point). The metadata must
/// come from [`fs::symlink_metadata`] so that the link itself is described, not its target.
#[cfg(windows)]
pub fn is_link(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Whether the entry is a link (see the Windows version).
#[cfg(not(windows))]
pub fn is_link(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Whether a link entry stands for a directory (directory symlink or junction).
#[cfg(windows)]
fn link_is_directory(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
}

#[cfg(not(windows))]
fn link_is_directory(_metadata: &Metadata) -> bool {
    false
}

/// Path of the temporary file used to write `target` atomically: `<target>.tmp`.
pub fn temp_path_for(target: &Path) -> PathBuf {
    let mut name: OsString = target.file_name().unwrap_or_default().to_owned();
    name.push(".tmp");
    target.with_file_name(name)
}

/// Writes `bytes` to `target` atomically: the content goes to a temporary file in the same
/// directory, which is flushed to disk then renamed over the target. On failure the target is left
/// untouched and the temporary file is removed.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = temp_path_for(target);
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Removes a link without following it. Directory links (junctions, directory symlinks) are
/// removed with `remove_dir`, which deletes the link only and never the target's content.
pub fn remove_link(path: &Path, metadata: &Metadata) -> io::Result<()> {
    if link_is_directory(metadata) {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

fn remove_file_forced(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            // Read-only files cannot be deleted on Windows: clear the attribute and retry.
            let mut permissions = fs::metadata(path)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
            fs::remove_file(path)
        }
        other => other,
    }
}

/// Recursively removes `path`. Links found on the way (at any depth, `path` included) are removed
/// themselves and never followed. A missing path is not an error.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if is_link(&metadata) {
        return remove_link(path, &metadata);
    }
    if !metadata.is_dir() {
        return remove_file_forced(path);
    }
    for entry in fs::read_dir(path)? {
        remove_tree(&entry?.path())?;
    }
    fs::remove_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_creates_and_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("a.json");
        write_atomic(&target, b"one").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"one");
        write_atomic(&target, b"two").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");
        assert!(!temp_path_for(&target).exists());
    }

    #[test]
    fn write_atomic_failing_on_the_temp_file_keeps_the_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("a.json");
        fs::write(&target, b"original").unwrap();
        // A directory where the temporary file should go makes creating it fail.
        fs::create_dir(temp_path_for(&target)).unwrap();

        assert!(write_atomic(&target, b"new").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert!(temp_path_for(&target).is_dir());
    }

    #[test]
    fn write_atomic_failing_on_the_rename_leaves_no_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        // The target is a non-empty directory: renaming a file over it fails.
        let target = tmp.path().join("a.json");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("x"), b"x").unwrap();

        assert!(write_atomic(&target, b"new").is_err());
        assert!(!temp_path_for(&target).exists());
        assert!(target.join("x").exists());
    }

    #[test]
    fn remove_tree_removes_everything_and_tolerates_missing_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("d");
        fs::create_dir_all(dir.join("a").join("b")).unwrap();
        fs::write(dir.join("a").join("b").join("f.txt"), b"x").unwrap();
        fs::write(dir.join("g.txt"), b"x").unwrap();

        remove_tree(&dir).unwrap();
        assert!(!dir.exists());
        remove_tree(&dir).unwrap();
    }

    #[test]
    fn remove_tree_removes_read_only_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("d");
        fs::create_dir(&dir).unwrap();
        let file = dir.join("ro.txt");
        fs::write(&file, b"x").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();

        remove_tree(&dir).unwrap();
        assert!(!dir.exists());
    }
}
