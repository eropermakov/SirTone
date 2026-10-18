use sirtone_core::error::{AppError, ErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration: {0}")]
    Migration(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid: {0}")]
    Invalid(String),
}

impl From<LibraryError> for AppError {
    fn from(e: LibraryError) -> Self {
        let code = match &e {
            LibraryError::Sqlite(rusqlite::Error::SqliteFailure(f, _))
                if matches!(
                    f.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                return AppError::new(ErrorCode::DbBusy).retryable().with_details(e);
            }
            LibraryError::Sqlite(rusqlite::Error::SqliteFailure(f, _))
                if matches!(
                    f.code,
                    rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
                ) =>
            {
                ErrorCode::DbCorrupt
            }
            LibraryError::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => {
                ErrorCode::PermissionDenied
            }
            LibraryError::Invalid(_) => ErrorCode::InvalidArgument,
            _ => ErrorCode::Internal,
        };
        AppError::new(code).with_details(e)
    }
}
