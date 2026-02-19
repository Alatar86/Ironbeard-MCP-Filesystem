use rmcp::model::{ErrorCode, ErrorData};
use std::borrow::Cow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("Access denied: {path}")]
    PathDenied { path: String },

    #[error("Not found: {path}")]
    NotFound { path: String },

    #[error("Not a file: {path}")]
    NotAFile { path: String },

    #[error("Not a directory: {path}")]
    NotADirectory { path: String },

    #[error("File too large: {path} ({size} bytes, max {max} bytes)")]
    FileTooLarge { path: String, size: u64, max: u64 },

    #[error("Binary file detected: {path}. Use get_file_info to inspect its metadata.")]
    BinaryFile { path: String },

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("Invalid pattern: {0}")]
    PatternError(String),

    #[error("Edit failed on {path}: {reason}")]
    EditFailed { path: String, reason: String },
}

impl From<FsError> for ErrorData {
    fn from(err: FsError) -> Self {
        let code = match &err {
            FsError::NotFound { .. } => ErrorCode::RESOURCE_NOT_FOUND,
            FsError::IoError(_) | FsError::EditFailed { .. } => ErrorCode::INTERNAL_ERROR,
            FsError::PathDenied { .. }
            | FsError::NotAFile { .. }
            | FsError::NotADirectory { .. }
            | FsError::FileTooLarge { .. }
            | FsError::BinaryFile { .. }
            | FsError::PatternError(_) => ErrorCode::INVALID_PARAMS,
        };
        ErrorData {
            code,
            message: Cow::Owned(err.to_string()),
            data: None,
        }
    }
}

/// Converts an I/O error to a user-friendly message string.
/// Distinguishes OS-level "Permission denied" from our security PathDenied.
pub fn io_error_message(err: std::io::Error, path: &str) -> String {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        format!("Permission denied by operating system: {path}")
    } else {
        err.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_denied_maps_to_invalid_params() {
        let err = FsError::PathDenied {
            path: "/secret".into(),
        };
        let data: ErrorData = err.into();
        assert_eq!(data.code, ErrorCode::INVALID_PARAMS);
        assert!(data.message.contains("/secret"));
    }

    #[test]
    fn not_found_maps_to_resource_not_found() {
        let err = FsError::NotFound {
            path: "/missing".into(),
        };
        let data: ErrorData = err.into();
        assert_eq!(data.code, ErrorCode::RESOURCE_NOT_FOUND);
    }

    #[test]
    fn io_error_maps_to_internal_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "disk failure");
        let err: FsError = io_err.into();
        let data: ErrorData = err.into();
        assert_eq!(data.code, ErrorCode::INTERNAL_ERROR);
    }

    #[test]
    fn io_error_message_permission_denied() {
        let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let msg = super::io_error_message(err, "/some/path");
        assert!(msg.contains("Permission denied by operating system"));
        assert!(msg.contains("/some/path"));
    }

    #[test]
    fn io_error_message_other_error() {
        let err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let msg = super::io_error_message(err, "/some/path");
        assert!(msg.contains("file not found"));
        assert!(!msg.contains("Permission denied"));
    }
}
