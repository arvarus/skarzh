pub mod session;

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{Manager, State};

use session::{Session, SessionConfig, SessionError, SessionStatus, SessionStore, template};

/// Static information about the running application, exposed to the front end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// Builds the [`AppInfo`] from the crate metadata (`Cargo.toml`).
pub fn build_app_info() -> AppInfo {
    AppInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Tauri command returning the application name and version.
///
/// It is the single IPC command of the skeleton: it proves the front end can
/// reach the Rust backend.
#[tauri::command]
fn app_info() -> AppInfo {
    build_app_info()
}

/// The session store shared by the commands. Every session command takes the lock, so the
/// operations are serialised.
///
/// Long-running backend jobs (scan, thumbnails, hashing) must be cancelled and awaited before
/// `session_reset` runs, otherwise they would recreate files in the directory being wiped. There
/// are no such jobs yet: the ticket that adds them wires the cancellation.
pub struct SessionState(Mutex<SessionStore>);

impl SessionState {
    pub fn new(store: SessionStore) -> Self {
        Self(Mutex::new(store))
    }

    /// A panic while the lock was held cannot corrupt the store (it keeps no state in memory),
    /// so a poisoned lock is simply taken over.
    fn lock(&self) -> MutexGuard<'_, SessionStore> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Tells whether there is no session, a usable one, or an unreadable one.
#[tauri::command]
fn session_status(state: State<'_, SessionState>) -> Result<SessionStatus, SessionError> {
    Ok(state.lock().status())
}

/// Creates the session from the configuration (refused when one exists).
#[tauri::command]
fn session_create(
    state: State<'_, SessionState>,
    config: SessionConfig,
) -> Result<Session, SessionError> {
    state.lock().create(config)
}

/// Returns the saved session.
#[tauri::command]
fn session_get(state: State<'_, SessionState>) -> Result<Session, SessionError> {
    state.lock().get()
}

/// Returns the saved session to continue it.
#[tauri::command]
fn session_resume(state: State<'_, SessionState>) -> Result<Session, SessionError> {
    state.lock().resume()
}

/// Deletes all the application data of the session.
#[tauri::command]
fn session_reset(state: State<'_, SessionState>) -> Result<(), SessionError> {
    state.lock().reset()
}

/// Validates a path template and returns it normalised (`/` separators).
#[tauri::command]
fn validate_path_template(template: String) -> Result<String, SessionError> {
    Ok(template::validate(&template)?.normalized())
}

/// Validates a path template and renders it with a fixed sample (2024-07-14, "Canon EOS R6",
/// "Holidays", "IMG_0042.jpg").
#[tauri::command]
fn preview_path_template(template: String) -> Result<String, SessionError> {
    Ok(template::preview(&template)?)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let root = app.path().app_local_data_dir()?;
            app.manage(SessionState::new(SessionStore::new(root)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            session_status,
            session_create,
            session_get,
            session_resume,
            session_reset,
            validate_path_template,
            preview_path_template
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_matches_crate_metadata() {
        let info = build_app_info();
        assert_eq!(info.name, "skarzh");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn app_info_command_returns_the_same_value() {
        assert_eq!(app_info(), build_app_info());
    }

    #[test]
    fn template_commands_return_normalised_and_rendered_values() {
        assert_eq!(
            validate_path_template(r"{year}\{filename}".into()).unwrap(),
            "{year}/{filename}"
        );
        assert_eq!(
            preview_path_template("{year}/{month}/{filename}".into()).unwrap(),
            "2024/07/IMG_0042.jpg"
        );
        let error = preview_path_template("{year}".into()).unwrap_err();
        assert_eq!(error.code(), "template_missing_filename");
    }

    #[test]
    fn session_state_survives_a_poisoned_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let state = std::sync::Arc::new(SessionState::new(SessionStore::new(tmp.path())));
        let clone = state.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.lock();
            panic!("poison the lock");
        })
        .join();
        assert_eq!(state.lock().status(), SessionStatus::None);
    }

    #[test]
    fn app_info_serializes_to_the_shape_expected_by_the_front_end() {
        let json = serde_json::to_value(build_app_info()).unwrap();
        assert_eq!(json["name"], "skarzh");
        assert!(json["version"].is_string());
    }
}
