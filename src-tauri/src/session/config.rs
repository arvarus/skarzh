//! Data model of a session: the configuration chosen by the user and the persisted envelope.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::error::SessionError;

/// Version of the `session.json` schema written by this build.
///
/// A file with a higher version is never read nor overwritten (see
/// [`SessionError::SessionSchemaTooNew`]).
pub const SCHEMA_VERSION: u32 = 1;

/// The user's setup of a session. JSON keys are camelCase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    /// Folders containing the photos to process (at least one, never modified by the app).
    pub input_folders: Vec<PathBuf>,
    /// Folder receiving the organised photos (created when missing).
    pub output_folder: PathBuf,
    /// Path template, see [`super::template`].
    pub path_template: String,
}

/// A session as persisted in `session.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub schema_version: u32,
    pub config: SessionConfig,
    /// RFC 3339 UTC timestamp.
    pub created_at: String,
    /// RFC 3339 UTC timestamp.
    pub updated_at: String,
}

/// What the front end shows to offer resuming the previous session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub input_folders: Vec<PathBuf>,
    pub output_folder: PathBuf,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&Session> for SessionSummary {
    fn from(session: &Session) -> Self {
        Self {
            input_folders: session.config.input_folders.clone(),
            output_folder: session.config.output_folder.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
        }
    }
}

/// State of the session storage, tagged by `state` in JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SessionStatus {
    /// No session: the front end shows the setup.
    None,
    /// A readable session exists: offer to continue or start over.
    Existing { summary: SessionSummary },
    /// Something exists but cannot be used (corrupted, newer schema...). It is never
    /// overwritten: the front end can only offer a reset.
    Unreadable { reason: SessionError },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SessionConfig {
        SessionConfig {
            input_folders: vec![PathBuf::from(r"C:\photos")],
            output_folder: PathBuf::from(r"C:\out"),
            path_template: "{year}/{filename}".into(),
        }
    }

    #[test]
    fn config_uses_camel_case_keys() {
        let json = serde_json::to_value(config()).unwrap();
        assert_eq!(json["inputFolders"][0], r"C:\photos");
        assert_eq!(json["outputFolder"], r"C:\out");
        assert_eq!(json["pathTemplate"], "{year}/{filename}");
    }

    #[test]
    fn config_round_trips() {
        let text = serde_json::to_string(&config()).unwrap();
        assert_eq!(
            serde_json::from_str::<SessionConfig>(&text).unwrap(),
            config()
        );
    }

    #[test]
    fn status_json_shapes() {
        let none = serde_json::to_value(SessionStatus::None).unwrap();
        assert_eq!(none, serde_json::json!({ "state": "none" }));

        let session = Session {
            schema_version: SCHEMA_VERSION,
            config: config(),
            created_at: "2024-07-14T10:00:00Z".into(),
            updated_at: "2024-07-14T10:00:00Z".into(),
        };
        let existing = serde_json::to_value(SessionStatus::Existing {
            summary: SessionSummary::from(&session),
        })
        .unwrap();
        assert_eq!(existing["state"], "existing");
        assert_eq!(existing["summary"]["createdAt"], "2024-07-14T10:00:00Z");
        assert_eq!(existing["summary"]["outputFolder"], r"C:\out");

        let unreadable = serde_json::to_value(SessionStatus::Unreadable {
            reason: SessionError::SessionCorrupted("x".into()),
        })
        .unwrap();
        assert_eq!(unreadable["state"], "unreadable");
        assert_eq!(unreadable["reason"]["code"], "session_corrupted");
    }
}
