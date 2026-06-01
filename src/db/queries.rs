use sqlx::SqlitePool;

use super::models::*;

// Domain queries
pub async fn create_domain(pool: &SqlitePool, domain_name: &str) -> anyhow::Result<Domain> {
    let domain =
        sqlx::query_as::<_, Domain>("INSERT INTO domains (domain_name) VALUES (?) RETURNING *")
            .bind(domain_name)
            .fetch_one(pool)
            .await?;
    Ok(domain)
}

pub async fn get_domain_by_name(pool: &SqlitePool, name: &str) -> anyhow::Result<Option<Domain>> {
    let domain = sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE domain_name = ?")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(domain)
}

pub async fn list_domains(pool: &SqlitePool) -> anyhow::Result<Vec<Domain>> {
    let domains = sqlx::query_as::<_, Domain>("SELECT * FROM domains ORDER BY id")
        .fetch_all(pool)
        .await?;
    Ok(domains)
}

pub async fn delete_domain(pool: &SqlitePool, domain_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM domains WHERE id = ?")
        .bind(domain_id)
        .execute(pool)
        .await?;
    Ok(())
}

// User queries
pub async fn create_user(
    pool: &SqlitePool,
    email: &str,
    password_hash: &str,
    domain_id: i64,
    is_admin: bool,
) -> anyhow::Result<User> {
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, password_hash, domain_id, is_admin) VALUES (?, ?, ?, ?) RETURNING *",
    )
    .bind(email)
    .bind(password_hash)
    .bind(domain_id)
    .bind(is_admin)
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn get_user_by_email(pool: &SqlitePool, email: &str) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn get_user_by_id(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn list_users(pool: &SqlitePool) -> anyhow::Result<Vec<User>> {
    let users = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY id")
        .fetch_all(pool)
        .await?;
    Ok(users)
}

pub async fn delete_user(pool: &SqlitePool, user_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// Email queries
#[allow(clippy::too_many_arguments)]
pub async fn save_email(
    pool: &SqlitePool,
    message_id: Option<&str>,
    sender: &str,
    recipients: &str,
    subject: Option<&str>,
    body_text: Option<&str>,
    body_html: Option<&str>,
    raw_message: Option<&[u8]>,
    user_id: i64,
    mailbox: &str,
) -> anyhow::Result<Email> {
    let email = sqlx::query_as::<_, Email>(
        r#"INSERT INTO emails (message_id, sender, recipients, subject, body_text, body_html, raw_message, user_id, mailbox)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *"#,
    )
    .bind(message_id)
    .bind(sender)
    .bind(recipients)
    .bind(subject)
    .bind(body_text)
    .bind(body_html)
    .bind(raw_message)
    .bind(user_id)
    .bind(mailbox)
    .fetch_one(pool)
    .await?;
    Ok(email)
}

pub async fn get_emails_by_user(
    pool: &SqlitePool,
    user_id: i64,
    mailbox: &str,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<Email>> {
    let emails = sqlx::query_as::<_, Email>(
        "SELECT * FROM emails WHERE user_id = ? AND mailbox = ? AND is_deleted = FALSE ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(user_id)
    .bind(mailbox)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(emails)
}

pub async fn get_email_by_id(pool: &SqlitePool, email_id: i64) -> anyhow::Result<Option<Email>> {
    let email = sqlx::query_as::<_, Email>("SELECT * FROM emails WHERE id = ?")
        .bind(email_id)
        .fetch_optional(pool)
        .await?;
    Ok(email)
}

pub async fn mark_email_read(pool: &SqlitePool, email_id: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE emails SET is_read = TRUE WHERE id = ?")
        .bind(email_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_email_auth(
    pool: &SqlitePool,
    email_id: i64,
    spf_result: Option<&str>,
    dkim_signature: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE emails SET spf_result = ?, dkim_signature = ? WHERE id = ?")
        .bind(spf_result)
        .bind(dkim_signature)
        .bind(email_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_email(pool: &SqlitePool, email_id: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE emails SET is_deleted = TRUE WHERE id = ?")
        .bind(email_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn move_email(pool: &SqlitePool, email_id: i64, mailbox: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE emails SET mailbox = ? WHERE id = ?")
        .bind(mailbox)
        .bind(email_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn count_emails_by_user(
    pool: &SqlitePool,
    user_id: i64,
    mailbox: &str,
) -> anyhow::Result<i64> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM emails WHERE user_id = ? AND mailbox = ? AND is_deleted = FALSE",
    )
    .bind(user_id)
    .bind(mailbox)
    .fetch_one(pool)
    .await?;
    Ok(count.0)
}

pub async fn search_emails(
    pool: &SqlitePool,
    user_id: i64,
    query: &str,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<Email>> {
    let pattern = format!("%{}%", query);
    let emails = sqlx::query_as::<_, Email>(
        "SELECT * FROM emails WHERE user_id = ? AND is_deleted = FALSE
         AND (subject LIKE ? OR sender LIKE ? OR body_text LIKE ?)
         ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(user_id)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(emails)
}

pub async fn count_search_emails(
    pool: &SqlitePool,
    user_id: i64,
    query: &str,
) -> anyhow::Result<i64> {
    let pattern = format!("%{}%", query);
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM emails WHERE user_id = ? AND is_deleted = FALSE
         AND (subject LIKE ? OR sender LIKE ? OR body_text LIKE ?)",
    )
    .bind(user_id)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .fetch_one(pool)
    .await?;
    Ok(count.0)
}

pub async fn get_mailbox_counts(
    pool: &SqlitePool,
    user_id: i64,
) -> anyhow::Result<Vec<(String, i64, i64)>> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT mailbox, COUNT(*) as total, SUM(CASE WHEN is_read = FALSE THEN 1 ELSE 0 END) as unread
         FROM emails WHERE user_id = ? AND is_deleted = FALSE GROUP BY mailbox",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update_user_password(
    pool: &SqlitePool,
    user_id: i64,
    password_hash: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(password_hash)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// Attachment queries
pub async fn save_attachment(
    pool: &SqlitePool,
    email_id: i64,
    filename: Option<&str>,
    content_type: Option<&str>,
    data: &[u8],
) -> anyhow::Result<Attachment> {
    let attachment = sqlx::query_as::<_, Attachment>(
        "INSERT INTO attachments (email_id, filename, content_type, data, size) VALUES (?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(email_id)
    .bind(filename)
    .bind(content_type)
    .bind(data)
    .bind(data.len() as i64)
    .fetch_one(pool)
    .await?;
    Ok(attachment)
}

pub async fn get_attachments_by_email(
    pool: &SqlitePool,
    email_id: i64,
) -> anyhow::Result<Vec<Attachment>> {
    let attachments =
        sqlx::query_as::<_, Attachment>("SELECT * FROM attachments WHERE email_id = ?")
            .bind(email_id)
            .fetch_all(pool)
            .await?;
    Ok(attachments)
}

pub async fn get_attachment_by_id(
    pool: &SqlitePool,
    attachment_id: i64,
) -> anyhow::Result<Option<Attachment>> {
    let attachment = sqlx::query_as::<_, Attachment>("SELECT * FROM attachments WHERE id = ?")
        .bind(attachment_id)
        .fetch_optional(pool)
        .await?;
    Ok(attachment)
}
