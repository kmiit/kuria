use thiserror::Error;

#[derive(Error, Debug)]
pub enum KuriaError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("SMTP error: {0}")]
    Smtp(String),

    #[error("IMAP error: {0}")]
    Imap(String),

    #[error("Mail parse error: {0}")]
    MailParse(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("DNS error: {0}")]
    Dns(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, KuriaError>;
