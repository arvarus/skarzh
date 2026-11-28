//! Persistence of the session: `session.json` plus the cache directories, and the operations the
//! front end drives (status, create, get, resume, reset).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::config::{SCHEMA_VERSION, Session, SessionConfig, SessionStatus, SessionSummary};
use super::error::SessionError;
use super::fsutil;
use super::paths::{self, SESSION_DIR_NAME, SessionPaths};
use super::validation;

/// Reads and writes the one session of the application.
///
/// The store owns no state besides its paths: every call reads the disk. Callers serialise access
/// to it (the Tauri layer keeps it behind a `Mutex`).
///
/// Long-running backend jobs (scan, thumbnails, hashing) write under [`SessionPaths::dir`]. They
/// must be cancelled and awaited by their owner before [`SessionStore::reset`] is called, or they
/// would recreate files inside a directory that was just wiped. The jobs do not exist yet: the
/// backend ticket that adds them is responsible for wiring this.
#[derive(Debug, Clone)]
pub struct SessionStore {
    paths: SessionPaths,
}

impl SessionStore {
    /// `root` is the application local data directory: the session lives in `<root>/session`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            paths: SessionPaths::new(root),
        }
    }

    /// The locations of the session files, for the modules that store derived data.
    pub fn paths(&self) -> &SessionPaths {
        &self.paths
    }

    /// Tells whether there is no session, a usable one, or something that cannot be used.
    pub fn status(&self) -> SessionStatus {
        if fs::symlink_metadata(self.paths.dir())
            .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
        {
            return SessionStatus::None;
        }
        match self.load() {
            Ok(session) => SessionStatus::Existing {
                summary: SessionSummary::from(&session),
            },
            Err(reason) => SessionStatus::Unreadable { reason },
        }
    }

    /// Creates the session: validates the configuration, creates the output folder, the session
    /// directory with its `thumbnails/` and `cache/` subdirectories, and finally `session.json`
    /// (so the file only exists once the session is complete).
    ///
    /// Refused when anything exists in the session directory, even unreadable: the caller must
    /// [`reset`](Self::reset) first.
    pub fn create(&self, config: SessionConfig) -> Result<Session, SessionError> {
        if !matches!(self.status(), SessionStatus::None) {
            return Err(SessionError::SessionAlreadyExists);
        }
        let config = validation::validate_config(&self.paths, &config)?;
        let now = now_rfc3339()?;
        let session = Session {
            schema_version: SCHEMA_VERSION,
            config,
            created_at: now.clone(),
            updated_at: now,
        };
        let bytes = serde_json::to_vec_pretty(&session)
            .map_err(|e| SessionError::Io(format!("cannot serialise the session: {e}")))?;

        validation::ensure_output_folder(&session.config.output_folder)?;
        fs::create_dir_all(self.paths.root())
            .map_err(|e| SessionError::io(self.paths.root().display(), &e))?;
        fs::create_dir(self.paths.dir()).map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => SessionError::SessionAlreadyExists,
            _ => SessionError::io(self.paths.dir().display(), &e),
        })?;
        let written = fs::create_dir(self.paths.thumbnails_dir())
            .and_then(|()| fs::create_dir(self.paths.cache_dir()))
            .and_then(|()| fsutil::write_atomic(&self.paths.file(), &bytes));
        if let Err(error) = written {
            // Do not leave a half-built session behind: it would show up as unreadable.
            let _ = fsutil::remove_tree(self.paths.dir());
            return Err(SessionError::io(self.paths.dir().display(), &error));
        }
        Ok(session)
    }

    /// Loads the saved session.
    pub fn get(&self) -> Result<Session, SessionError> {
        self.load()
    }

    /// Loads the saved session to continue it. Resuming changes nothing on disk: `updated_at` is
    /// left as it was.
    pub fn resume(&self) -> Result<Session, SessionError> {
        self.load()
    }

    /// Deletes the whole session directory (settings, caches, hashes, scan data), whatever its
    /// state, and returns to "no session". Idempotent. The input and output folders are never
    /// touched. See [`remove_session_dir`] for the safety checks.
    pub fn reset(&self) -> Result<(), SessionError> {
        let protected = self.configured_folders();
        remove_session_dir(self.paths.root(), self.paths.dir(), &protected)
    }

    fn load(&self) -> Result<Session, SessionError> {
        let bytes = match fs::read(self.paths.file()) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(if fs::symlink_metadata(self.paths.dir()).is_ok() {
                    SessionError::SessionFileMissing
                } else {
                    SessionError::NoSession
                });
            }
            Err(e) => return Err(SessionError::io(self.paths.file().display(), &e)),
        };
        let corrupted = |e: &dyn std::fmt::Display| SessionError::SessionCorrupted(e.to_string());
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| corrupted(&e))?;
        let version = value
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .filter(|version| *version >= 1)
            .ok_or_else(|| corrupted(&"missing or invalid schemaVersion"))?;
        if version > u64::from(SCHEMA_VERSION) {
            return Err(SessionError::SessionSchemaTooNew {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        let session: Session = serde_json::from_value(value).map_err(|e| corrupted(&e))?;
        if session.config.input_folders.is_empty() {
            return Err(corrupted(&"no input folder"));
        }
        Ok(session)
    }

    /// Best effort list of the user's folders recorded in `session.json`, read leniently so it
    /// works for sessions that [`load`](Self::load) rejects (corrupted elsewhere, newer schema).
    fn configured_folders(&self) -> Vec<PathBuf> {
        let Ok(bytes) = fs::read(self.paths.file()) else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return Vec::new();
        };
        let config = &value["config"];
        let mut folders: Vec<PathBuf> = config["inputFolders"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .collect();
        if let Some(output) = config["outputFolder"].as_str() {
            folders.push(PathBuf::from(output));
        }
        folders
    }
}

fn now_rfc3339() -> Result<String, SessionError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| SessionError::Io(format!("cannot format the date: {e}")))
}

/// Deletes the session directory `dir` after checking that it is safe to do so.
///
/// Refused with [`SessionError::ResetUnsafe`] (nothing is touched) unless:
/// - `root` and `dir` are absolute and `root` is not a filesystem root;
/// - `dir` is exactly `<root>/session` (names compared case-insensitively after removing `.` and
///   `..`);
/// - `dir` is neither an ancestor nor a descendant of any `protected` folder (the configured input
///   and output folders; links resolved).
///
/// Links are never followed: if `dir` itself is a symlink or junction only the link is removed,
/// and links found inside are removed without touching their targets. A missing directory is not
/// an error.
pub(crate) fn remove_session_dir(
    root: &Path,
    dir: &Path,
    protected: &[PathBuf],
) -> Result<(), SessionError> {
    let refuse = |reason: &str| SessionError::ResetUnsafe(format!("{}: {reason}", dir.display()));

    if !root.is_absolute() || !dir.is_absolute() {
        return Err(refuse("the path is not absolute"));
    }
    if root.parent().is_none() || dir.parent().is_none() {
        return Err(refuse("the path is a filesystem root"));
    }
    let expected = paths::lexical_normalize(root).join(SESSION_DIR_NAME);
    if !paths::same_path(&paths::lexical_normalize(dir), &expected) {
        return Err(refuse("it is not the session directory of the store"));
    }

    let location = paths::resolve(root).join(SESSION_DIR_NAME);
    for folder in protected {
        if paths::overlaps(&paths::resolve(folder), &location) {
            return Err(refuse(&format!(
                "it overlaps the configured folder {}",
                folder.display()
            )));
        }
    }

    match fsutil::remove_tree(dir) {
        Ok(()) => Ok(()),
        Err(e) => Err(SessionError::io(dir.display(), &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::template::TemplateError;
    #[cfg(windows)]
    use std::process::Command;
    use tempfile::TempDir;

    struct Fixture {
        tmp: TempDir,
        store: SessionStore,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let store = SessionStore::new(tmp.path().join("appdata"));
            Self { tmp, store }
        }

        /// Creates a folder with a photo inside.
        fn folder_with_photo(&self, name: &str) -> PathBuf {
            let dir = self.tmp.path().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("IMG_0001.jpg"), name.as_bytes()).unwrap();
            dir
        }

        fn config(&self) -> SessionConfig {
            SessionConfig {
                input_folders: vec![self.folder_with_photo("in1"), self.folder_with_photo("in2")],
                output_folder: self.folder_with_photo("out"),
                path_template: "{year}/{month}/{filename}".into(),
            }
        }

        fn session_dir(&self) -> PathBuf {
            self.store.paths().dir().to_path_buf()
        }
    }

    fn assert_photo(dir: &Path, name: &str) {
        assert_eq!(fs::read(dir.join("IMG_0001.jpg")).unwrap(), name.as_bytes());
    }

    #[test]
    fn status_is_none_without_session() {
        let f = Fixture::new();
        assert_eq!(f.store.status(), SessionStatus::None);
        assert_eq!(f.store.get(), Err(SessionError::NoSession));
        assert_eq!(f.store.resume(), Err(SessionError::NoSession));
    }

    #[test]
    fn create_get_resume_round_trip() {
        let f = Fixture::new();
        let config = f.config();
        let created = f.store.create(config.clone()).unwrap();

        assert_eq!(created.schema_version, SCHEMA_VERSION);
        assert_eq!(created.config, config);
        assert_eq!(created.created_at, created.updated_at);
        let date = OffsetDateTime::parse(&created.created_at, &Rfc3339).unwrap();
        assert_eq!(date.offset(), time::UtcOffset::UTC);
        assert!(created.created_at.ends_with('Z'));

        assert_eq!(f.store.get().unwrap(), created);
        assert_eq!(f.store.resume().unwrap(), created);
        assert_eq!(f.store.resume().unwrap().updated_at, created.updated_at);
        match f.store.status() {
            SessionStatus::Existing { summary } => {
                assert_eq!(summary, SessionSummary::from(&created));
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn create_builds_the_directory_layout() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let paths = f.store.paths();
        assert!(paths.file().is_file());
        assert!(paths.thumbnails_dir().is_dir());
        assert!(paths.cache_dir().is_dir());
        assert!(!fsutil::temp_path_for(&paths.file()).exists());
        assert_eq!(paths.dir(), f.tmp.path().join("appdata").join("session"));
    }

    #[test]
    fn create_normalises_the_template_and_creates_the_output() {
        let f = Fixture::new();
        let mut config = f.config();
        config.output_folder = f.tmp.path().join("new").join("out");
        config.path_template = r"{year}\{filename}".into();
        let created = f.store.create(config.clone()).unwrap();
        assert_eq!(created.config.path_template, "{year}/{filename}");
        assert!(config.output_folder.is_dir());
        assert_eq!(
            f.store.get().unwrap().config.path_template,
            "{year}/{filename}"
        );
    }

    #[test]
    fn session_json_uses_camel_case() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let json: Value =
            serde_json::from_slice(&fs::read(f.store.paths().file()).unwrap()).unwrap();
        assert_eq!(json["schemaVersion"], 1);
        assert!(json["config"]["inputFolders"].is_array());
        assert!(json["config"]["outputFolder"].is_string());
        assert!(json["config"]["pathTemplate"].is_string());
        assert!(json["createdAt"].is_string());
        assert!(json["updatedAt"].is_string());
    }

    #[test]
    fn create_is_refused_when_a_session_exists_and_leaves_it_untouched() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let before = fs::read(f.store.paths().file()).unwrap();

        let mut other = f.config();
        other.path_template = "{filename}".into();
        assert_eq!(
            f.store.create(other),
            Err(SessionError::SessionAlreadyExists)
        );
        assert_eq!(fs::read(f.store.paths().file()).unwrap(), before);
    }

    #[test]
    fn create_is_refused_over_an_unreadable_session_and_never_overwrites_it() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        fs::write(f.store.paths().file(), b"{ not json").unwrap();

        assert_eq!(
            f.store.create(f.config()),
            Err(SessionError::SessionAlreadyExists)
        );
        assert_eq!(fs::read(f.store.paths().file()).unwrap(), b"{ not json");
    }

    #[test]
    fn invalid_config_creates_nothing() {
        let f = Fixture::new();
        let mut config = f.config();
        config.path_template = "{year}".into();
        assert_eq!(
            f.store.create(config),
            Err(SessionError::Template(TemplateError::MissingFilename))
        );
        assert!(!f.session_dir().exists());
        assert_eq!(f.store.status(), SessionStatus::None);

        let mut config = f.config();
        config.input_folders.push(f.tmp.path().join("missing"));
        config.output_folder = f.tmp.path().join("must-not-exist");
        assert!(matches!(
            f.store.create(config.clone()),
            Err(SessionError::InputNotFound(_))
        ));
        assert!(!config.output_folder.exists());
        assert!(!f.session_dir().exists());
    }

    #[test]
    fn status_reports_a_corrupted_file() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        for content in [
            &b"{ not json"[..],
            b"",
            b"[]",
            b"{\"schemaVersion\": \"one\"}",
            b"{\"schemaVersion\": 0}",
            b"{\"schemaVersion\": 1}",
            b"\xff\xfe",
        ] {
            fs::write(f.store.paths().file(), content).unwrap();
            match f.store.status() {
                SessionStatus::Unreadable { reason } => {
                    assert_eq!(reason.code(), "session_corrupted", "{content:?}");
                }
                other => panic!("unexpected status {other:?} for {content:?}"),
            }
            assert!(matches!(
                f.store.get(),
                Err(SessionError::SessionCorrupted(_))
            ));
        }
    }

    #[test]
    fn status_reports_a_session_without_input_folder_as_corrupted() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let mut json: Value =
            serde_json::from_slice(&fs::read(f.store.paths().file()).unwrap()).unwrap();
        json["config"]["inputFolders"] = serde_json::json!([]);
        fs::write(f.store.paths().file(), serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(matches!(
            f.store.get(),
            Err(SessionError::SessionCorrupted(_))
        ));
    }

    #[test]
    fn status_reports_a_newer_schema_and_never_overwrites_it() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let newer = br#"{"schemaVersion": 2, "somethingNew": true}"#;
        fs::write(f.store.paths().file(), newer).unwrap();

        assert_eq!(
            f.store.status(),
            SessionStatus::Unreadable {
                reason: SessionError::SessionSchemaTooNew {
                    found: 2,
                    supported: 1
                }
            }
        );
        assert_eq!(
            f.store.get(),
            Err(SessionError::SessionSchemaTooNew {
                found: 2,
                supported: 1
            })
        );
        assert_eq!(
            f.store.create(f.config()),
            Err(SessionError::SessionAlreadyExists)
        );
        assert_eq!(fs::read(f.store.paths().file()).unwrap(), newer);
    }

    #[test]
    fn status_reports_a_missing_session_file() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        fs::remove_file(f.store.paths().file()).unwrap();
        assert_eq!(
            f.store.status(),
            SessionStatus::Unreadable {
                reason: SessionError::SessionFileMissing
            }
        );
        assert_eq!(f.store.get(), Err(SessionError::SessionFileMissing));
    }

    #[test]
    fn reset_removes_the_session_directory_and_everything_below() {
        let f = Fixture::new();
        let config = f.config();
        f.store.create(config.clone()).unwrap();
        let paths = f.store.paths();
        fs::write(paths.thumbnails_dir().join("a.jpg"), b"thumb").unwrap();
        fs::create_dir_all(paths.cache_dir().join("hashes").join("deep")).unwrap();
        fs::write(
            paths.cache_dir().join("hashes").join("deep").join("h.bin"),
            b"h",
        )
        .unwrap();
        fs::write(paths.dir().join("scan.json"), b"{}").unwrap();

        f.store.reset().unwrap();

        assert!(!f.session_dir().exists());
        assert_eq!(f.store.status(), SessionStatus::None);
        // The parent (app data) directory itself stays.
        assert!(f.tmp.path().join("appdata").is_dir());
        // The user's folders are untouched.
        assert_photo(&config.input_folders[0], "in1");
        assert_photo(&config.input_folders[1], "in2");
        assert_photo(&config.output_folder, "out");
    }

    #[test]
    fn reset_is_idempotent() {
        let f = Fixture::new();
        f.store.reset().unwrap();
        f.store.create(f.config()).unwrap();
        f.store.reset().unwrap();
        f.store.reset().unwrap();
        assert_eq!(f.store.status(), SessionStatus::None);
    }

    #[test]
    fn a_new_session_can_be_created_after_a_reset() {
        let f = Fixture::new();
        let first = f.store.create(f.config()).unwrap();
        f.store.reset().unwrap();
        let mut config = f.config();
        config.path_template = "{filename}".into();
        let second = f.store.create(config.clone()).unwrap();
        assert_eq!(second.config, config);
        assert_ne!(second.config, first.config);
    }

    #[test]
    fn reset_works_on_a_corrupted_session() {
        let f = Fixture::new();
        let config = f.config();
        f.store.create(config.clone()).unwrap();
        fs::write(f.store.paths().file(), b"garbage").unwrap();
        fs::write(f.store.paths().cache_dir().join("x"), b"x").unwrap();

        f.store.reset().unwrap();
        assert_eq!(f.store.status(), SessionStatus::None);
        assert_photo(&config.input_folders[0], "in1");
    }

    #[test]
    fn reset_works_on_a_partially_deleted_session() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        fs::remove_file(f.store.paths().file()).unwrap();
        fs::remove_dir(f.store.paths().cache_dir()).unwrap();
        f.store.reset().unwrap();
        assert!(!f.session_dir().exists());

        // Only an empty session directory left.
        fs::create_dir_all(f.session_dir()).unwrap();
        f.store.reset().unwrap();
        assert!(!f.session_dir().exists());
    }

    #[test]
    fn reset_removes_read_only_files() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let file = f.store.paths().cache_dir().join("ro.bin");
        fs::write(&file, b"x").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();
        f.store.reset().unwrap();
        assert!(!f.session_dir().exists());
    }

    #[test]
    fn reset_refuses_a_path_that_is_not_the_session_directory() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let root = f.tmp.path().join("appdata");
        let precious = root.join("other");
        fs::create_dir_all(&precious).unwrap();
        fs::write(precious.join("keep.txt"), b"keep").unwrap();

        let candidates = [
            root.clone(),
            precious.clone(),
            root.join("session").join(".."),
            root.join("session").join("cache"),
            root.join("sessions"),
            f.tmp.path().to_path_buf(),
            PathBuf::from("session"),
            PathBuf::from(r"relative\session"),
        ];
        for candidate in candidates {
            let result = remove_session_dir(&root, &candidate, &[]);
            assert!(
                matches!(result, Err(SessionError::ResetUnsafe(_))),
                "{candidate:?}: {result:?}"
            );
        }
        assert_eq!(fs::read(precious.join("keep.txt")).unwrap(), b"keep");
        assert!(f.store.paths().file().is_file());
        assert!(f.store.paths().cache_dir().is_dir());
    }

    #[test]
    fn reset_refuses_filesystem_roots() {
        let drive = PathBuf::from(r"C:\");
        for (root, dir) in [
            (drive.clone(), drive.clone()),
            (drive.clone(), drive.join("session")),
            (drive.join("data"), drive.clone()),
        ] {
            let result = remove_session_dir(&root, &dir, &[]);
            assert!(
                matches!(result, Err(SessionError::ResetUnsafe(_))),
                "{root:?} {dir:?}: {result:?}"
            );
        }
    }

    #[test]
    fn reset_accepts_the_session_directory_spelled_differently() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let root = f.tmp.path().join("appdata");
        let spelled = root.join("SESSION").join(".").join("..").join("Session");
        remove_session_dir(&root, &spelled, &[]).unwrap();
        assert!(!f.session_dir().exists());
    }

    #[test]
    fn reset_refuses_when_a_configured_folder_overlaps_the_session_directory() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let root = f.tmp.path().join("appdata");
        let marker = f.store.paths().cache_dir().join("marker");
        fs::write(&marker, b"m").unwrap();

        let outside = f.folder_with_photo("outside");
        let overlapping = [
            root.clone(),
            f.session_dir(),
            f.store.paths().thumbnails_dir(),
            // Spelled differently: case and a dot segment.
            root.join("SESSION").join("cache"),
            f.tmp.path().to_path_buf(),
        ];
        for folder in overlapping {
            let result =
                remove_session_dir(&root, &f.session_dir(), &[outside.clone(), folder.clone()]);
            assert!(
                matches!(result, Err(SessionError::ResetUnsafe(_))),
                "{folder:?}: {result:?}"
            );
            assert_eq!(fs::read(&marker).unwrap(), b"m");
        }
    }

    #[test]
    fn reset_refuses_when_session_json_lists_an_overlapping_folder() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let mut json: Value =
            serde_json::from_slice(&fs::read(f.store.paths().file()).unwrap()).unwrap();
        // Hand-edited file: the output is the application data directory.
        json["config"]["outputFolder"] =
            Value::String(f.tmp.path().join("appdata").to_string_lossy().into_owned());
        fs::write(f.store.paths().file(), serde_json::to_vec(&json).unwrap()).unwrap();

        assert!(matches!(f.store.reset(), Err(SessionError::ResetUnsafe(_))));
        assert!(f.store.paths().file().is_file());

        // The folders are read even from a file this version cannot otherwise use.
        json["schemaVersion"] = serde_json::json!(99);
        fs::write(f.store.paths().file(), serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(matches!(f.store.reset(), Err(SessionError::ResetUnsafe(_))));
    }

    #[cfg(windows)]
    fn make_junction(link: &Path, target: &Path) {
        let output = Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .expect("cmd is available on Windows");
        assert!(
            output.status.success(),
            "mklink /J failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[cfg(windows)]
    #[test]
    fn reset_does_not_follow_a_junction_inside_the_session_directory() {
        let f = Fixture::new();
        let config = f.config();
        f.store.create(config.clone()).unwrap();
        let target = f.folder_with_photo("junction-target");
        fs::create_dir_all(target.join("sub")).unwrap();
        fs::write(target.join("sub").join("deep.txt"), b"deep").unwrap();
        make_junction(&f.store.paths().cache_dir().join("link"), &target);
        // Sanity check: the junction really leads to the target.
        assert_photo(&f.store.paths().cache_dir().join("link"), "junction-target");

        f.store.reset().unwrap();

        assert!(!f.session_dir().exists());
        assert_photo(&target, "junction-target");
        assert_eq!(
            fs::read(target.join("sub").join("deep.txt")).unwrap(),
            b"deep"
        );
        assert_photo(&config.input_folders[0], "in1");
    }

    #[cfg(windows)]
    #[test]
    fn reset_removes_a_junction_used_as_the_session_directory_without_following_it() {
        let f = Fixture::new();
        let target = f.folder_with_photo("elsewhere");
        fs::create_dir_all(target.join("cache")).unwrap();
        fs::write(target.join("cache").join("precious.bin"), b"p").unwrap();
        fs::create_dir_all(f.tmp.path().join("appdata")).unwrap();
        make_junction(&f.session_dir(), &target);

        f.store.reset().unwrap();

        assert!(
            fs::symlink_metadata(f.session_dir()).is_err(),
            "link removed"
        );
        assert_photo(&target, "elsewhere");
        assert_eq!(
            fs::read(target.join("cache").join("precious.bin")).unwrap(),
            b"p"
        );
        assert_eq!(f.store.status(), SessionStatus::None);
    }

    #[cfg(windows)]
    #[test]
    fn reset_does_not_follow_a_symlink_inside_the_session_directory() {
        let f = Fixture::new();
        f.store.create(f.config()).unwrap();
        let target = f.folder_with_photo("symlink-target");
        let file_target = target.join("IMG_0001.jpg");
        let dir_link = f.store.paths().cache_dir().join("dirlink");
        let file_link = f.store.paths().cache_dir().join("filelink.jpg");
        if let Err(error) = std::os::windows::fs::symlink_dir(&target, &dir_link) {
            eprintln!("SKIPPED: cannot create symlinks in this environment ({error})");
            return;
        }
        std::os::windows::fs::symlink_file(&file_target, &file_link).unwrap();

        f.store.reset().unwrap();

        assert!(!f.session_dir().exists());
        assert_photo(&target, "symlink-target");
    }
}
