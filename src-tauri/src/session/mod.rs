//! The session: the user's setup plus everything the application derives from it.
//!
//! skarzh works on one session at a time. At launch the user continues the previous session or
//! starts a new one, which deletes all the application data of the session (settings, caches,
//! hashes, scan data). The input and output folders belong to the user and are never modified or
//! deleted by the session logic.
//!
//! Layout under the application local data directory:
//!
//! ```text
//! session/
//!   session.json   schema version, configuration, created_at, updated_at
//!   thumbnails/    reserved for the thumbnail cache
//!   cache/         reserved for hashes, scan data, review decisions...
//! ```
//!
//! Everything derived from a session must live under that directory so that a reset wipes it.
//!
//! The logic here is plain Rust, independent of Tauri and unit-tested with temporary
//! directories; the Tauri commands in `lib.rs` are thin wrappers.

mod config;
mod error;
mod fsutil;
mod paths;
mod store;
pub mod template;
mod validation;

pub use config::{SCHEMA_VERSION, Session, SessionConfig, SessionStatus, SessionSummary};
pub use error::SessionError;
pub use paths::SessionPaths;
pub use store::SessionStore;
