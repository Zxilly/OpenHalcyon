use std::path::PathBuf;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use halcyon_storage::Error as StorageError;
use halcyon_types::HostError;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct PlatformConfig {
    pub database_path: PathBuf,
    pub public_address: String,
    pub seats_per_match: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("match host unavailable")]
    HostUnavailable,
    #[error("storage unavailable: {0}")]
    Database(StorageError),
    #[error("platform worker unavailable")]
    WorkerUnavailable,
}

impl From<StorageError> for PlatformError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Unauthorized => Self::Unauthorized,
            StorageError::AlreadyLinked => Self::Conflict("account already linked"),
            independent => Self::Database(independent),
        }
    }
}

impl From<HostError> for PlatformError {
    fn from(error: HostError) -> Self {
        match error {
            HostError::Unauthorized => Self::Unauthorized,
            HostError::Invalid(message) => Self::Invalid(message),
            HostError::Conflict(message) => Self::Conflict(message),
            HostError::HostUnavailable => Self::HostUnavailable,
            HostError::Database(message) => {
                tracing::error!(error = %message, "host database request failed");
                Self::WorkerUnavailable
            }
            HostError::WorkerUnavailable => Self::WorkerUnavailable,
        }
    }
}

impl From<PlatformError> for HostError {
    fn from(error: PlatformError) -> Self {
        match error {
            PlatformError::Unauthorized => Self::Unauthorized,
            PlatformError::Invalid(message) => Self::Invalid(message),
            PlatformError::Conflict(message) => Self::Conflict(message),
            PlatformError::HostUnavailable => Self::HostUnavailable,
            PlatformError::Database(error) => Self::Database(error.to_string()),
            PlatformError::WorkerUnavailable => Self::WorkerUnavailable,
        }
    }
}

impl From<diesel::result::Error> for PlatformError {
    fn from(error: diesel::result::Error) -> Self {
        Self::Database(halcyon_storage::Error::Query(error))
    }
}

impl IntoResponse for PlatformError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "authentication required"),
            Self::Invalid(message) => (StatusCode::BAD_REQUEST, message),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::HostUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "match host unavailable"),
            Self::Database(_) | Self::WorkerUnavailable => {
                tracing::error!(error = %self, "platform request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "platform unavailable")
            }
        };
        #[derive(Serialize)]
        struct ErrorResponse {
            error: &'static str,
        }
        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

pub type Result<T> = std::result::Result<T, PlatformError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_error_maps_losslessly_across_platform_boundary() {
        assert!(matches!(
            PlatformError::from(HostError::Unauthorized),
            PlatformError::Unauthorized
        ));
        assert!(matches!(
            PlatformError::from(HostError::Invalid("bad")),
            PlatformError::Invalid("bad")
        ));
        assert!(matches!(
            PlatformError::from(HostError::Conflict("busy")),
            PlatformError::Conflict("busy")
        ));
        assert!(matches!(
            PlatformError::from(HostError::HostUnavailable),
            PlatformError::HostUnavailable
        ));
        assert!(matches!(
            PlatformError::from(HostError::WorkerUnavailable),
            PlatformError::WorkerUnavailable
        ));
        assert!(matches!(
            PlatformError::from(HostError::Database("locked".into())),
            PlatformError::WorkerUnavailable
        ));
        assert_eq!(
            HostError::from(PlatformError::Conflict("taken")),
            HostError::Conflict("taken")
        );
    }
}
