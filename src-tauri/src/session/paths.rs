//! Locations inside the session directory and path comparison helpers.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Name of the session directory under the root given to the store.
pub const SESSION_DIR_NAME: &str = "session";
/// Name of the session file inside the session directory.
pub const SESSION_FILE_NAME: &str = "session.json";
/// Name of the thumbnail cache directory (reserved for the scan/thumbnail backend).
pub const THUMBNAILS_DIR_NAME: &str = "thumbnails";
/// Name of the general cache directory (hashes, scan data...; reserved for the backend).
pub const CACHE_DIR_NAME: &str = "cache";

/// Everything the application derives from a session lives under [`SessionPaths::dir`], so that
/// resetting the session wipes it all. Backend modules must only write there (and in the output
/// folder when the user asks for it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPaths {
    root: PathBuf,
    dir: PathBuf,
}

impl SessionPaths {
    /// `root` is the application local data directory (a temp directory in tests).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let dir = root.join(SESSION_DIR_NAME);
        Self { root, dir }
    }

    /// The directory the session directory is created in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The session directory: `<root>/session`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `<root>/session/session.json`.
    pub fn file(&self) -> PathBuf {
        self.dir.join(SESSION_FILE_NAME)
    }

    /// `<root>/session/thumbnails`.
    pub fn thumbnails_dir(&self) -> PathBuf {
        self.dir.join(THUMBNAILS_DIR_NAME)
    }

    /// `<root>/session/cache`.
    pub fn cache_dir(&self) -> PathBuf {
        self.dir.join(CACHE_DIR_NAME)
    }
}

/// Removes `.` and resolves `..` without touching the file system.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Absolute form of a path that may not exist yet: the deepest existing ancestor is
/// canonicalised (links resolved) and the missing tail is appended unchanged.
pub fn resolve(path: &Path) -> PathBuf {
    let normalized = lexical_normalize(path);
    let mut tail = Vec::new();
    let mut current = normalized.as_path();
    loop {
        if let Ok(canonical) = fs::canonicalize(current) {
            let mut resolved = canonical;
            resolved.extend(tail.iter().rev());
            return resolved;
        }
        match (current.file_name(), current.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                current = parent;
            }
            _ => return normalized,
        }
    }
}

fn fold(component: Component<'_>) -> String {
    component.as_os_str().to_string_lossy().to_lowercase()
}

/// Whether `child` is `parent` or below it, comparing components case-insensitively (Windows).
/// Both paths should come from [`resolve`] or [`lexical_normalize`].
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let child: Vec<String> = child.components().map(fold).collect();
    let parent: Vec<String> = parent.components().map(fold).collect();
    child.len() >= parent.len() && child[..parent.len()] == parent[..]
}

/// Whether the two paths are the same location (case-insensitive).
pub fn same_path(a: &Path, b: &Path) -> bool {
    is_within(a, b) && is_within(b, a)
}

/// Whether one of the two paths contains the other (or they are equal).
pub fn overlaps(a: &Path, b: &Path) -> bool {
    is_within(a, b) || is_within(b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_follow_the_layout() {
        let paths = SessionPaths::new(r"C:\data\app");
        assert_eq!(paths.root(), Path::new(r"C:\data\app"));
        assert_eq!(paths.dir(), Path::new(r"C:\data\app\session"));
        assert_eq!(paths.file(), Path::new(r"C:\data\app\session\session.json"));
        assert_eq!(
            paths.thumbnails_dir(),
            Path::new(r"C:\data\app\session\thumbnails")
        );
        assert_eq!(paths.cache_dir(), Path::new(r"C:\data\app\session\cache"));
    }

    #[test]
    fn lexical_normalize_handles_dots() {
        assert_eq!(
            lexical_normalize(Path::new(r"C:\a\.\b\..\c")),
            PathBuf::from(r"C:\a\c")
        );
        assert_eq!(
            lexical_normalize(Path::new(r"C:\..\a")),
            PathBuf::from(r"C:\a")
        );
    }

    #[test]
    fn comparison_is_case_insensitive_and_component_based() {
        assert!(is_within(
            Path::new(r"C:\Photos\2024"),
            Path::new(r"c:\photos")
        ));
        assert!(is_within(Path::new(r"C:\photos"), Path::new(r"C:\PHOTOS\")));
        assert!(!is_within(
            Path::new(r"C:\photos2"),
            Path::new(r"C:\photos")
        ));
        assert!(!is_within(
            Path::new(r"C:\photos"),
            Path::new(r"C:\photos\a")
        ));
        assert!(same_path(Path::new(r"C:\A\B"), Path::new(r"c:\a\b")));
        assert!(overlaps(Path::new(r"C:\a"), Path::new(r"C:\a\b")));
        assert!(overlaps(Path::new(r"C:\a\b"), Path::new(r"C:\a")));
        assert!(!overlaps(Path::new(r"C:\a"), Path::new(r"C:\b")));
    }

    #[test]
    fn resolve_keeps_the_missing_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(tmp.path()).unwrap();
        let resolved = resolve(&tmp.path().join("missing").join("deeper"));
        assert_eq!(resolved, canonical.join("missing").join("deeper"));
        assert_eq!(resolve(tmp.path()), canonical);
    }
}
