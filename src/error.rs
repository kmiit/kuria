use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum KuriaError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("SMTP error: {0}")]
    Smtp(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
