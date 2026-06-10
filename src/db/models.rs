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
    pub api_enabled: bool,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiToken {
    pub id: i64,
    pub user_id: i64,
    pub token: String,
    pub name: String,
    pub last_used_at: Option<NaiveDateTime>,
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
pub struct EmailSummary {
    pub id: i64,
    pub sender: String,
    pub recipients: String, // JSON array
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub is_read: bool,
    pub mailbox: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub attachment_count: i64,
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OutboundQueueItem {
    pub id: i64,
    pub envelope_sender: String,
    pub recipients: String,
    pub raw_message: Vec<u8>,
    pub attempts: i64,
    pub max_attempts: i64,
    pub status: String,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
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

#[derive(Debug, Deserialize)]
pub struct SendEmailRequest {
    pub to: Vec<String>,
    pub cc: Option<Vec<String>>,
    pub bcc: Option<Vec<String>>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Option<Vec<SendEmailAttachmentRequest>>,
    pub draft_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SendEmailAttachmentRequest {
    pub filename: String,
    pub content_type: Option<String>,
    pub data_base64: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiTokenRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserApiAccessRequest {
    pub api_enabled: bool,
}
