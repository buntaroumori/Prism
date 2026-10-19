//! Central error type for the Prism backend.
//!
//! Every fallible backend operation returns [`PrismError`]. Tauri commands
//! require their error type to be `serde::Serialize`, so the type carries a
//! stable machine-readable `code` (used by the frontend to branch on failure
//! modes) and a human-readable `message`.

use std::path::{Path, PathBuf};

use serde::ser::{Serialize, SerializeStruct, Serializer};

/// Errors surfaced to the UI or logged by the backend.
#[derive(Debug, thiserror::Error)]
pub enum PrismError {
    /// `SQLite` / `sqlx` failure (query, pool acquisition, decode).
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// A migration failed to apply. This is fatal: the schema state is
    /// unknown and the application must not continue writing data.
    #[error("database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    /// Filesystem failure with the offending path attached.
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Tauri runtime / path resolution failure.
    #[error("desktop runtime error: {0}")]
    Tauri(#[from] tauri::Error),

    /// The bundled application resources are inconsistent (missing migration
    /// set, unreadable icon, ...). Signals a broken installation.
    #[error("invalid installation: {0}")]
    Install(String),

    /// A command received input that failed validation. The message is safe to
    /// display verbatim in the UI.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The audio engine refused the operation (device busy, unsupported
    /// format, decoder failure). The message is user-facing.
    #[error("audio engine error: {0}")]
    Audio(#[from] crate::audio::AudioError),

    /// Any other internal invariant violation. Never contains user data.
    #[error("internal error: {0}")]
    Internal(String),
}

impl PrismError {
    /// Stable error code consumed by the TypeScript layer.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "database",
            Self::Migration(_) => "migration",
            Self::Io { .. } => "io",
            Self::Tauri(_) => "runtime",
            Self::Install(_) => "install",
            Self::Audio(_) => "audio",
            Self::InvalidInput(_) => "invalid_input",
            Self::Internal(_) => "internal",
        }
    }

    /// Attach a path to a raw `std::io::Error` without losing the source.
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Convenience constructor for validation failures.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

impl Serialize for PrismError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("PrismError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

/// `anyhow` bridge for code paths that only need context propagation before
/// being converted at the command boundary.
impl From<anyhow::Error> for PrismError {
    fn from(value: anyhow::Error) -> Self {
        Self::Internal(format!("{value:#}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_keep_the_path() {
        let err = PrismError::io("/tmp/library", std::io::Error::other("disk gone"));
        assert_eq!(err.code(), "io");
        assert!(err.to_string().contains("/tmp/library"));
        assert!(err.to_string().contains("disk gone"));
    }

    #[test]
    fn serializes_as_code_and_message() {
        let err = PrismError::invalid("bit depth must be 16, 24 or 32");
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["code"], "invalid_input");
        assert!(json["message"].as_str().unwrap().contains("bit depth"));
    }
}
