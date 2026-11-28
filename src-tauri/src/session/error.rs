//! Typed errors of the session module.
//!
//! Every error serialises to `{ "code": "...", "message": "..." }`. The `code` is stable and meant
//! to be switched on by the front end; the `message` is an English description for logs and
//! fallback display.

use std::io;
use std::path::PathBuf;

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};

use super::template::TemplateError;

/// Everything that can go wrong in a session operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("at least one input folder is required")]
    NoInputFolders,
    #[error("the path must be absolute: {}", .0.display())]
    PathNotAbsolute(PathBuf),
    #[error("input folder not found: {}", .0.display())]
    InputNotFound(PathBuf),
    #[error("input path is not a folder: {}", .0.display())]
    InputNotADirectory(PathBuf),
    #[error("input folder listed more than once: {}", .0.display())]
    DuplicateInput(PathBuf),
    #[error("input folder {} is inside input folder {}", .inner.display(), .outer.display())]
    NestedInputs { outer: PathBuf, inner: PathBuf },
    #[error("output folder {} is inside input folder {}", .output.display(), .input.display())]
    OutputInsideInput { output: PathBuf, input: PathBuf },
    #[error("input folder {} is inside output folder {}", .input.display(), .output.display())]
    InputInsideOutput { input: PathBuf, output: PathBuf },
    #[error("output path is not a folder: {}", .0.display())]
    OutputNotADirectory(PathBuf),
    #[error("cannot create the output folder {}: {reason}", .path.display())]
    OutputCreateFailed { path: PathBuf, reason: String },
    #[error("the folder overlaps the application session data: {}", .0.display())]
    SessionDirConflict(PathBuf),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error("a session already exists: reset it first")]
    SessionAlreadyExists,
    #[error("there is no session")]
    NoSession,
    #[error("the session directory exists but session.json is missing")]
    SessionFileMissing,
    #[error("session.json is corrupted: {0}")]
    SessionCorrupted(String),
    #[error("session.json uses schema version {found}, this version supports up to {supported}")]
    SessionSchemaTooNew { found: u64, supported: u32 },
    #[error("refusing to delete the session directory: {0}")]
    ResetUnsafe(String),
    #[error("i/o error: {0}")]
    Io(String),
}

impl SessionError {
    /// Wraps an i/o error with the operation it happened in.
    pub(crate) fn io(context: impl std::fmt::Display, error: &io::Error) -> Self {
        Self::Io(format!("{context}: {error}"))
    }

    /// Stable machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoInputFolders => "no_input_folders",
            Self::PathNotAbsolute(_) => "path_not_absolute",
            Self::InputNotFound(_) => "input_not_found",
            Self::InputNotADirectory(_) => "input_not_a_directory",
            Self::DuplicateInput(_) => "duplicate_input",
            Self::NestedInputs { .. } => "nested_inputs",
            Self::OutputInsideInput { .. } => "output_inside_input",
            Self::InputInsideOutput { .. } => "input_inside_output",
            Self::OutputNotADirectory(_) => "output_not_a_directory",
            Self::OutputCreateFailed { .. } => "output_create_failed",
            Self::SessionDirConflict(_) => "session_dir_conflict",
            Self::Template(error) => error.code(),
            Self::SessionAlreadyExists => "session_already_exists",
            Self::NoSession => "no_session",
            Self::SessionFileMissing => "session_file_missing",
            Self::SessionCorrupted(_) => "session_corrupted",
            Self::SessionSchemaTooNew { .. } => "session_schema_too_new",
            Self::ResetUnsafe(_) => "reset_unsafe",
            Self::Io(_) => "io",
        }
    }
}

impl Serialize for SessionError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("SessionError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_to_code_and_message() {
        let json = serde_json::to_value(SessionError::NoSession).unwrap();
        assert_eq!(json["code"], "no_session");
        assert_eq!(json["message"], "there is no session");
    }

    #[test]
    fn template_errors_keep_their_own_code() {
        let error = SessionError::from(TemplateError::Empty);
        assert_eq!(error.code(), "template_empty");
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "template_empty");
    }

    #[test]
    fn path_in_message() {
        let error = SessionError::InputNotFound(PathBuf::from(r"C:\nope"));
        assert!(error.to_string().contains(r"C:\nope"));
    }
}
