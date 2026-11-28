//! Validation of a [`SessionConfig`] before a session is created.
//!
//! Folders are compared after resolving links and `..` (see [`paths::resolve`]) and
//! case-insensitively, as Windows paths are.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::config::SessionConfig;
use super::error::SessionError;
use super::paths::{self, SessionPaths};
use super::template;

/// Checks the configuration against the rules below and returns it with the path template
/// normalised (`/` separators). Nothing is created on disk.
///
/// - at least one input folder; every folder path is absolute;
/// - every input exists and is a folder;
/// - no input is listed twice, and no input is inside another input;
/// - the output is not inside an input, and no input is inside the output (an output equal to an
///   input counts as inside);
/// - the output is not an existing file;
/// - neither the session directory, nor anything below it, nor anything above it, is used as an
///   input or as the output (resetting the session deletes that directory);
/// - the path template is valid.
pub fn validate_config(
    session: &SessionPaths,
    config: &SessionConfig,
) -> Result<SessionConfig, SessionError> {
    if config.input_folders.is_empty() {
        return Err(SessionError::NoInputFolders);
    }
    let path_template = template::validate(&config.path_template)?.normalized();

    let mut inputs: Vec<(&PathBuf, PathBuf)> = Vec::new();
    for input in &config.input_folders {
        require_absolute(input)?;
        let metadata = fs::metadata(input).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => SessionError::InputNotFound(input.clone()),
            _ => SessionError::io(input.display(), &error),
        })?;
        if !metadata.is_dir() {
            return Err(SessionError::InputNotADirectory(input.clone()));
        }
        inputs.push((input, paths::resolve(input)));
    }

    for (index, (original, resolved)) in inputs.iter().enumerate() {
        for (other_original, other_resolved) in &inputs[..index] {
            if paths::same_path(resolved, other_resolved) {
                return Err(SessionError::DuplicateInput((*original).clone()));
            }
            if paths::is_within(resolved, other_resolved) {
                return Err(SessionError::NestedInputs {
                    outer: (*other_original).clone(),
                    inner: (*original).clone(),
                });
            }
            if paths::is_within(other_resolved, resolved) {
                return Err(SessionError::NestedInputs {
                    outer: (*original).clone(),
                    inner: (*other_original).clone(),
                });
            }
        }
    }

    let output = &config.output_folder;
    require_absolute(output)?;
    if fs::metadata(output).is_ok_and(|metadata| !metadata.is_dir()) {
        return Err(SessionError::OutputNotADirectory(output.clone()));
    }
    let resolved_output = paths::resolve(output);
    for (original, resolved) in &inputs {
        if paths::is_within(&resolved_output, resolved) {
            return Err(SessionError::OutputInsideInput {
                output: output.clone(),
                input: (*original).clone(),
            });
        }
        if paths::is_within(resolved, &resolved_output) {
            return Err(SessionError::InputInsideOutput {
                input: (*original).clone(),
                output: output.clone(),
            });
        }
    }

    let session_dir = paths::resolve(session.dir());
    for (original, resolved) in &inputs {
        if paths::overlaps(resolved, &session_dir) {
            return Err(SessionError::SessionDirConflict((*original).clone()));
        }
    }
    if paths::overlaps(&resolved_output, &session_dir) {
        return Err(SessionError::SessionDirConflict(output.clone()));
    }

    Ok(SessionConfig {
        input_folders: config.input_folders.clone(),
        output_folder: config.output_folder.clone(),
        path_template,
    })
}

/// Creates the output folder (and its missing parents) when it does not exist.
pub fn ensure_output_folder(output: &Path) -> Result<(), SessionError> {
    fs::create_dir_all(output).map_err(|error| SessionError::OutputCreateFailed {
        path: output.to_path_buf(),
        reason: error.to_string(),
    })
}

fn require_absolute(path: &Path) -> Result<(), SessionError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(SessionError::PathNotAbsolute(path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::template::TemplateError;
    use tempfile::TempDir;

    struct Fixture {
        tmp: TempDir,
        session: SessionPaths,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let session = SessionPaths::new(tmp.path().join("appdata"));
            Self { tmp, session }
        }

        fn dir(&self, relative: &str) -> PathBuf {
            let path = self.tmp.path().join(relative);
            fs::create_dir_all(&path).unwrap();
            path
        }

        fn config(&self, inputs: &[&Path], output: &Path) -> SessionConfig {
            SessionConfig {
                input_folders: inputs.iter().map(|p| p.to_path_buf()).collect(),
                output_folder: output.to_path_buf(),
                path_template: "{year}/{filename}".into(),
            }
        }

        fn validate(&self, config: &SessionConfig) -> Result<SessionConfig, SessionError> {
            validate_config(&self.session, config)
        }
    }

    #[test]
    fn accepts_a_valid_config_and_normalises_the_template() {
        let f = Fixture::new();
        let (a, b) = (f.dir("a"), f.dir("b"));
        let out = f.tmp.path().join("out").join("not-created-yet");
        let mut config = f.config(&[&a, &b], &out);
        config.path_template = r"{year}\{month}\{filename}".into();

        let validated = f.validate(&config).unwrap();
        assert_eq!(validated.path_template, "{year}/{month}/{filename}");
        assert_eq!(validated.input_folders, config.input_folders);
        assert!(!out.exists(), "validation must not create anything");
    }

    #[test]
    fn rejects_no_input() {
        let f = Fixture::new();
        let out = f.dir("out");
        assert_eq!(
            f.validate(&f.config(&[], &out)),
            Err(SessionError::NoInputFolders)
        );
    }

    #[test]
    fn rejects_relative_paths() {
        let f = Fixture::new();
        let out = f.dir("out");
        let config = f.config(&[Path::new("photos")], &out);
        assert_eq!(
            f.validate(&config),
            Err(SessionError::PathNotAbsolute(PathBuf::from("photos")))
        );
        let a = f.dir("a");
        let config = f.config(&[&a], Path::new("out"));
        assert_eq!(
            f.validate(&config),
            Err(SessionError::PathNotAbsolute(PathBuf::from("out")))
        );
    }

    #[test]
    fn rejects_a_missing_input() {
        let f = Fixture::new();
        let (a, out) = (f.dir("a"), f.dir("out"));
        let missing = f.tmp.path().join("missing");
        let config = f.config(&[&a, &missing], &out);
        assert_eq!(
            f.validate(&config),
            Err(SessionError::InputNotFound(missing))
        );
    }

    #[test]
    fn rejects_an_input_that_is_a_file() {
        let f = Fixture::new();
        let out = f.dir("out");
        let file = f.tmp.path().join("photo.jpg");
        fs::write(&file, b"x").unwrap();
        assert_eq!(
            f.validate(&f.config(&[&file], &out)),
            Err(SessionError::InputNotADirectory(file))
        );
    }

    #[test]
    fn rejects_duplicate_inputs_including_case_variants_and_dot_segments() {
        let f = Fixture::new();
        let (a, out) = (f.dir("Photos"), f.dir("out"));
        assert_eq!(
            f.validate(&f.config(&[&a, &a], &out)),
            Err(SessionError::DuplicateInput(a.clone()))
        );

        let upper = PathBuf::from(a.to_string_lossy().to_uppercase());
        let lower = PathBuf::from(a.to_string_lossy().to_lowercase());
        assert_eq!(
            f.validate(&f.config(&[&upper, &lower], &out)),
            Err(SessionError::DuplicateInput(lower))
        );

        let dotted = a.join("..").join("Photos");
        assert_eq!(
            f.validate(&f.config(&[&a, &dotted], &out)),
            Err(SessionError::DuplicateInput(dotted))
        );
    }

    #[test]
    fn rejects_nested_inputs_in_both_orders() {
        let f = Fixture::new();
        let (outer, out) = (f.dir("outer"), f.dir("out"));
        let inner = f.dir("outer/inner");
        assert_eq!(
            f.validate(&f.config(&[&outer, &inner], &out)),
            Err(SessionError::NestedInputs {
                outer: outer.clone(),
                inner: inner.clone()
            })
        );
        assert_eq!(
            f.validate(&f.config(&[&inner, &outer], &out)),
            Err(SessionError::NestedInputs { outer, inner })
        );
    }

    #[test]
    fn sibling_folders_with_a_common_name_prefix_are_not_nested() {
        let f = Fixture::new();
        let (a, b, out) = (f.dir("photos"), f.dir("photos2"), f.dir("out"));
        assert!(f.validate(&f.config(&[&a, &b], &out)).is_ok());
    }

    #[test]
    fn rejects_an_output_inside_an_input() {
        let f = Fixture::new();
        let a = f.dir("a");
        let out = a.join("sorted");
        assert_eq!(
            f.validate(&f.config(&[&a], &out)),
            Err(SessionError::OutputInsideInput {
                output: out,
                input: a.clone()
            })
        );
    }

    #[test]
    fn rejects_an_output_equal_to_an_input() {
        let f = Fixture::new();
        let a = f.dir("a");
        assert_eq!(
            f.validate(&f.config(&[&a], &a)),
            Err(SessionError::OutputInsideInput {
                output: a.clone(),
                input: a.clone()
            })
        );
    }

    #[test]
    fn rejects_an_input_inside_the_output() {
        let f = Fixture::new();
        let out = f.dir("out");
        let a = f.dir("out/photos");
        assert_eq!(
            f.validate(&f.config(&[&a], &out)),
            Err(SessionError::InputInsideOutput {
                input: a.clone(),
                output: out
            })
        );
    }

    #[test]
    fn rejects_an_output_that_is_a_file() {
        let f = Fixture::new();
        let a = f.dir("a");
        let out = f.tmp.path().join("out.txt");
        fs::write(&out, b"x").unwrap();
        assert_eq!(
            f.validate(&f.config(&[&a], &out)),
            Err(SessionError::OutputNotADirectory(out))
        );
    }

    #[test]
    fn rejects_the_session_directory_and_its_surroundings() {
        let f = Fixture::new();
        let (a, out) = (f.dir("a"), f.dir("out"));
        let session_dir = f.dir("appdata/session");
        let below = f.dir("appdata/session/thumbnails");
        let root = f.tmp.path().join("appdata");

        for forbidden in [&session_dir, &below, &root] {
            assert_eq!(
                f.validate(&f.config(&[forbidden], &out)),
                Err(SessionError::SessionDirConflict(forbidden.clone())),
                "input {forbidden:?}"
            );
            assert_eq!(
                f.validate(&f.config(&[&a], forbidden)),
                Err(SessionError::SessionDirConflict(forbidden.clone())),
                "output {forbidden:?}"
            );
        }
    }

    #[test]
    fn rejects_the_session_directory_even_when_it_does_not_exist_yet() {
        let f = Fixture::new();
        let (a, out) = (f.dir("a"), f.dir("out"));
        let session_dir = f.session.dir().to_path_buf();
        assert!(!session_dir.exists());
        assert_eq!(
            f.validate(&f.config(&[&a], &session_dir.join("cache"))),
            Err(SessionError::SessionDirConflict(session_dir.join("cache")))
        );
        assert!(f.validate(&f.config(&[&a], &out)).is_ok());
    }

    #[test]
    fn rejects_an_invalid_template() {
        let f = Fixture::new();
        let (a, out) = (f.dir("a"), f.dir("out"));
        let mut config = f.config(&[&a], &out);
        config.path_template = "{year}".into();
        assert_eq!(
            f.validate(&config),
            Err(SessionError::Template(TemplateError::MissingFilename))
        );
    }

    #[test]
    fn ensure_output_folder_creates_missing_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("a").join("b");
        ensure_output_folder(&out).unwrap();
        assert!(out.is_dir());
        ensure_output_folder(&out).unwrap();
    }

    #[test]
    fn ensure_output_folder_reports_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        fs::write(&file, b"x").unwrap();
        let error = ensure_output_folder(&file.join("sub")).unwrap_err();
        assert_eq!(error.code(), "output_create_failed");
    }
}
