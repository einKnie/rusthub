use std::error::Error;

/// Database Error
#[derive(Debug)]
pub enum DatabaseError {
    NoConnection,
    Failed,
    Duplicate,
    GeneralError(Box<dyn Error>),
}

impl Error for DatabaseError {}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseError::NoConnection => write!(f, "No connection to database server"),
            DatabaseError::Failed => write!(f, "Database query failed"),
            DatabaseError::Duplicate => write!(f, "Database entry exists"),
            DatabaseError::GeneralError(e) => write!(f, "General database error: {e:?}"),
        }
    }
}
