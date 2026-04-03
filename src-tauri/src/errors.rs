use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    UpstreamUnavailable(String),
    #[error("{0}")]
    ImportFailed(String),
    #[error("{0}")]
    ParseFailed(String),
    #[error("{0}")]
    SchemaInvalid(String),
    #[error("{0}")]
    Internal(String),
}

#[derive(Debug, Serialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let payload = match self {
            Self::Validation(message) => ErrorPayload {
                code: "VALIDATION_ERROR".into(),
                message: message.clone(),
            },
            Self::NotFound(message) => ErrorPayload {
                code: "NOT_FOUND".into(),
                message: message.clone(),
            },
            Self::UpstreamUnavailable(message) => ErrorPayload {
                code: "UPSTREAM_UNAVAILABLE".into(),
                message: message.clone(),
            },
            Self::ImportFailed(message) => ErrorPayload {
                code: "PAPER_IMPORT_FAILED".into(),
                message: message.clone(),
            },
            Self::ParseFailed(message) => ErrorPayload {
                code: "PAPER_PARSE_FAILED".into(),
                message: message.clone(),
            },
            Self::SchemaInvalid(message) => ErrorPayload {
                code: "AGENT_SCHEMA_INVALID".into(),
                message: message.clone(),
            },
            Self::Internal(message) => ErrorPayload {
                code: "INTERNAL_ERROR".into(),
                message: message.clone(),
            },
        };

        payload.serialize(serializer)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(error: reqwest::Error) -> Self {
        Self::UpstreamUnavailable(error.to_string())
    }
}
