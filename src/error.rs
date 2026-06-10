use thiserror::Error;

#[derive(Error, Debug)]
pub enum KuriaError {
    #[error("SMTP error: {0}")]
    Smtp(String),
}
