use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Domain {
    pub id: i64,
    pub domain_name: String,
    pub dkim_private_key: Option<String>,
    pub dkim_public_key: Option<String>,
    pub dkim_selector: Option<String>,
    pub spf_record: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub domain_id: i64,
    pub is_admin: bool,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Email {
    pub id: i64,
    pub message_id: Option<String>,
    pub sender: String,
    pub recipients: String, // JSON array
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub raw_message: Option<Vec<u8>>,
    pub dkim_signature: Option<String>,
    pub spf_result: Option<String>,
    pub dmarc_result: Option<String>,
    pub is_read: bool,
    pub is_deleted: bool,
    pub mailbox: Option<String>,
    pub user_id: i64,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Attachment {
    pub id: i64,
    pub email_id: i64,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Option<Vec<u8>>,
    pub size: Option<i64>,
}

// Request/Response models for the web API

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    pub domain_id: i64,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateDomainRequest {
    pub domain_name: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: User,
}

#[derive(Debug, Deserialize)]
pub struct SendEmailRequest {
    pub to: Vec<String>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EmailListResponse {
    pub emails: Vec<Email>,
    pub total: i64,
}
