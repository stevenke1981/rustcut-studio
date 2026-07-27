use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RustCutError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("asset not found: {0}")]
    AssetNotFound(String),

    #[error("track not found: {0}")]
    TrackNotFound(String),

    #[error("clip not found: {0}")]
    ClipNotFound(String),

    #[error("invalid request: {0}")]
    Validation(String),

    #[error("external command `{program}` failed with status {status}: {stderr}")]
    CommandFailed {
        program: String,
        status: String,
        stderr: String,
    },

    #[error("required executable was not found: {program}")]
    ExecutableNotFound { program: String },

    #[error("path is outside the project directory: {0}")]
    UnsafePath(PathBuf),

    #[error("planner response was invalid: {0}")]
    Planner(String),

    #[error("transcription response was invalid: {0}")]
    Transcription(String),
}

pub type Result<T> = std::result::Result<T, RustCutError>;
