use serde::Serialize;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
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
    fn app_info_serializes_to_the_shape_expected_by_the_front_end() {
        let json = serde_json::to_value(build_app_info()).unwrap();
        assert_eq!(json["name"], "skarzh");
        assert!(json["version"].is_string());
    }
}
