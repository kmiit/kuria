use sqlx::SqlitePool;

use super::models::*;

pub const OUTBOUND_STATUS_QUEUED: &str = "queued";
pub const OUTBOUND_STATUS_SENT: &str = "sent";
pub const OUTBOUND_STATUS_FAILED: &str = "failed";

// System settings
pub async fn get_system_setting(pool: &SqlitePool, key: &str) -> anyhow::Result<Option<String>> {
    let value = sqlx::query_scalar::<_, String>("SELECT value FROM system_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(value)
}

pub async fn set_system_setting(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO system_settings (key, value, updated_at)
        VALUES (?, ?, CURRENT_TIMESTAMP)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

// Domain queries
pub async fn create_domain(pool: &SqlitePool, domain_name: &str) -> anyhow::Result<Domain> {
    let spf_record = crate::mail::auth::generate_spf_record(domain_name, &[]);
    let domain = sqlx::query_as::<_, Domain>(
        "INSERT INTO domains (domain_name, spf_record) VALUES (?, ?) RETURNING *",
    )
    .bind(domain_name)
    .bind(spf_record)
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

pub async fn get_domain_by_id(pool: &SqlitePool, domain_id: i64) -> anyhow::Result<Option<Domain>> {
    let domain = sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE id = ?")
        .bind(domain_id)
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

pub async fn count_users_by_domain(pool: &SqlitePool, domain_id: i64) -> anyhow::Result<i64> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE domain_id = ?")
        .bind(domain_id)
        .fetch_one(pool)
        .await?;
    Ok(count.0)
}

pub async fn update_domain_dkim(
    pool: &SqlitePool,
    domain_id: i64,
    selector: &str,
    private_key: &str,
    public_key: &str,
) -> anyhow::Result<Domain> {
    let domain = sqlx::query_as::<_, Domain>(
        r#"
        UPDATE domains
        SET dkim_selector = ?, dkim_private_key = ?, dkim_public_key = ?
        WHERE id = ?
        RETURNING *
        "#,
    )
    .bind(selector)
    .bind(private_key)
    .bind(public_key)
    .bind(domain_id)
    .fetch_one(pool)
    .await?;
    Ok(domain)
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

pub async fn delete_user(pool: &SqlitePool, user_id: i64) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "DELETE FROM attachments WHERE email_id IN (SELECT id FROM emails WHERE user_id = ?)",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM emails WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

// Email queries
pub struct NewEmail<'a> {
    pub message_id: Option<&'a str>,
    pub sender: &'a str,
    pub recipients: &'a str,
    pub subject: Option<&'a str>,
    pub body_text: Option<&'a str>,
    pub body_html: Option<&'a str>,
    pub raw_message: Option<&'a [u8]>,
    pub user_id: i64,
    pub mailbox: &'a str,
    pub is_read: bool,
}

pub struct DraftEmail<'a> {
    pub sender: &'a str,
    pub recipients: &'a str,
    pub subject: Option<&'a str>,
    pub body_text: Option<&'a str>,
    pub body_html: Option<&'a str>,
    pub raw_message: &'a [u8],
    pub user_id: i64,
}

pub async fn save_email(pool: &SqlitePool, email: NewEmail<'_>) -> anyhow::Result<Email> {
    let email = sqlx::query_as::<_, Email>(
        r#"INSERT INTO emails (message_id, sender, recipients, subject, body_text, body_html, raw_message, user_id, mailbox, is_read)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *"#,
    )
    .bind(email.message_id)
    .bind(email.sender)
    .bind(email.recipients)
    .bind(email.subject)
    .bind(email.body_text)
    .bind(email.body_html)
    .bind(email.raw_message)
    .bind(email.user_id)
    .bind(email.mailbox)
    .bind(email.is_read)
    .fetch_one(pool)
    .await?;
    Ok(email)
}

pub async fn save_draft(
    pool: &SqlitePool,
    draft_id: Option<i64>,
    draft: DraftEmail<'_>,
) -> anyhow::Result<Option<Email>> {
    if let Some(draft_id) = draft_id {
        let email = sqlx::query_as::<_, Email>(
            r#"
            UPDATE emails
            SET sender = ?,
                recipients = ?,
                subject = ?,
                body_text = ?,
                body_html = ?,
                raw_message = ?,
                mailbox = 'Drafts',
                is_read = TRUE,
                is_deleted = FALSE
            WHERE id = ? AND user_id = ? AND mailbox = 'Drafts'
            RETURNING *
            "#,
        )
        .bind(draft.sender)
        .bind(draft.recipients)
        .bind(draft.subject)
        .bind(draft.body_text)
        .bind(draft.body_html)
        .bind(draft.raw_message)
        .bind(draft_id)
        .bind(draft.user_id)
        .fetch_optional(pool)
        .await?;

        return Ok(email);
    }

    let email = save_email(
        pool,
        NewEmail {
            message_id: None,
            sender: draft.sender,
            recipients: draft.recipients,
            subject: draft.subject,
            body_text: draft.body_text,
            body_html: draft.body_html,
            raw_message: Some(draft.raw_message),
            user_id: draft.user_id,
            mailbox: "Drafts",
            is_read: true,
        },
    )
    .await?;

    Ok(Some(email))
}

pub async fn get_emails_by_user(
    pool: &SqlitePool,
    user_id: i64,
    mailbox: &str,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<EmailSummary>> {
    let emails = sqlx::query_as::<_, EmailSummary>(
        r#"
        SELECT
            emails.id,
            emails.sender,
            emails.recipients,
            emails.subject,
            emails.body_text,
            emails.is_read,
            emails.mailbox,
            emails.created_at,
            COUNT(attachments.id) AS attachment_count
        FROM emails
        LEFT JOIN attachments ON attachments.email_id = emails.id
        WHERE emails.user_id = ? AND emails.mailbox = ? AND emails.is_deleted = FALSE
        GROUP BY emails.id
        ORDER BY emails.created_at DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(user_id)
    .bind(mailbox)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(emails)
}

pub async fn get_emails_for_imap(
    pool: &SqlitePool,
    user_id: i64,
    mailbox: &str,
) -> anyhow::Result<Vec<Email>> {
    let emails = sqlx::query_as::<_, Email>(
        "SELECT * FROM emails WHERE user_id = ? AND mailbox = ? ORDER BY id ASC",
    )
    .bind(user_id)
    .bind(mailbox)
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

pub async fn set_email_read(pool: &SqlitePool, email_id: i64, is_read: bool) -> anyhow::Result<()> {
    sqlx::query("UPDATE emails SET is_read = ? WHERE id = ?")
        .bind(is_read)
        .bind(email_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_email_deleted(
    pool: &SqlitePool,
    email_id: i64,
    is_deleted: bool,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE emails SET is_deleted = ? WHERE id = ?")
        .bind(is_deleted)
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
    dmarc_result: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE emails SET spf_result = ?, dkim_signature = ?, dmarc_result = ? WHERE id = ?",
    )
    .bind(spf_result)
    .bind(dkim_signature)
    .bind(dmarc_result)
    .bind(email_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_email(pool: &SqlitePool, email_id: i64) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let current = sqlx::query_as::<_, (Option<String>, bool)>(
        "SELECT mailbox, is_deleted FROM emails WHERE id = ?",
    )
    .bind(email_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((mailbox, is_deleted)) = current else {
        tx.commit().await?;
        return Ok(());
    };

    if mailbox.as_deref() == Some("Trash") || is_deleted {
        sqlx::query("DELETE FROM attachments WHERE email_id = ?")
            .bind(email_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM emails WHERE id = ?")
            .bind(email_id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("UPDATE emails SET mailbox = ?, is_deleted = FALSE WHERE id = ?")
            .bind("Trash")
            .bind(email_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn permanently_delete_email(pool: &SqlitePool, email_id: i64) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM attachments WHERE email_id = ?")
        .bind(email_id)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query("DELETE FROM emails WHERE id = ?")
        .bind(email_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

pub async fn empty_trash(pool: &SqlitePool, user_id: i64) -> anyhow::Result<u64> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM attachments
        WHERE email_id IN (
            SELECT id FROM emails WHERE user_id = ? AND mailbox = 'Trash'
        )
        "#,
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    let result = sqlx::query("DELETE FROM emails WHERE user_id = ? AND mailbox = 'Trash'")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
}

pub async fn move_email(pool: &SqlitePool, email_id: i64, mailbox: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE emails SET mailbox = ? WHERE id = ?")
        .bind(mailbox)
        .bind(email_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn copy_email_to_mailbox(
    pool: &SqlitePool,
    email_id: i64,
    user_id: i64,
    mailbox: &str,
) -> anyhow::Result<Option<Email>> {
    let mut tx = pool.begin().await?;

    let source = sqlx::query_as::<_, Email>("SELECT * FROM emails WHERE id = ? AND user_id = ?")
        .bind(email_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;

    let Some(source) = source else {
        tx.commit().await?;
        return Ok(None);
    };

    let copied = sqlx::query_as::<_, Email>(
        r#"
        INSERT INTO emails (
            message_id, sender, recipients, subject, body_text, body_html, raw_message,
            dkim_signature, spf_result, dmarc_result, is_read, is_deleted, mailbox, user_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING *
        "#,
    )
    .bind(source.message_id.as_deref())
    .bind(&source.sender)
    .bind(&source.recipients)
    .bind(source.subject.as_deref())
    .bind(source.body_text.as_deref())
    .bind(source.body_html.as_deref())
    .bind(source.raw_message.as_deref())
    .bind(source.dkim_signature.as_deref())
    .bind(source.spf_result.as_deref())
    .bind(source.dmarc_result.as_deref())
    .bind(source.is_read)
    .bind(source.is_deleted)
    .bind(mailbox)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO attachments (email_id, filename, content_type, data, size)
        SELECT ?, filename, content_type, data, size
        FROM attachments
        WHERE email_id = ?
        "#,
    )
    .bind(copied.id)
    .bind(email_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(copied))
}

pub async fn expunge_deleted_emails(
    pool: &SqlitePool,
    user_id: i64,
    mailbox: &str,
) -> anyhow::Result<u64> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM attachments
        WHERE email_id IN (
            SELECT id FROM emails WHERE user_id = ? AND mailbox = ? AND is_deleted = TRUE
        )
        "#,
    )
    .bind(user_id)
    .bind(mailbox)
    .execute(&mut *tx)
    .await?;

    let result =
        sqlx::query("DELETE FROM emails WHERE user_id = ? AND mailbox = ? AND is_deleted = TRUE")
            .bind(user_id)
            .bind(mailbox)
            .execute(&mut *tx)
            .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
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
) -> anyhow::Result<Vec<EmailSummary>> {
    let pattern = like_contains_pattern(query);
    let emails = sqlx::query_as::<_, EmailSummary>(
        r#"
        SELECT
            emails.id,
            emails.sender,
            emails.recipients,
            emails.subject,
            emails.body_text,
            emails.is_read,
            emails.mailbox,
            emails.created_at,
            COUNT(attachments.id) AS attachment_count
        FROM emails
        LEFT JOIN attachments ON attachments.email_id = emails.id
        WHERE emails.user_id = ? AND emails.is_deleted = FALSE
          AND (
              emails.subject LIKE ? ESCAPE '\'
              OR emails.sender LIKE ? ESCAPE '\'
              OR emails.recipients LIKE ? ESCAPE '\'
              OR emails.body_text LIKE ? ESCAPE '\'
              OR emails.body_html LIKE ? ESCAPE '\'
          )
        GROUP BY emails.id
        ORDER BY emails.created_at DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(user_id)
    .bind(&pattern)
    .bind(&pattern)
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
    let pattern = like_contains_pattern(query);
    let count: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM emails
        WHERE user_id = ? AND is_deleted = FALSE
          AND (
              subject LIKE ? ESCAPE '\'
              OR sender LIKE ? ESCAPE '\'
              OR recipients LIKE ? ESCAPE '\'
              OR body_text LIKE ? ESCAPE '\'
              OR body_html LIKE ? ESCAPE '\'
          )
        "#,
    )
    .bind(user_id)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .fetch_one(pool)
    .await?;
    Ok(count.0)
}

fn like_contains_pattern(value: &str) -> String {
    let mut pattern = String::with_capacity(value.len() + 2);
    pattern.push('%');
    for ch in value.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

pub async fn get_mailbox_counts(
    pool: &SqlitePool,
    user_id: i64,
) -> anyhow::Result<Vec<(String, i64, i64)>> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT mailbox, COUNT(*) as total, SUM(CASE WHEN is_read = FALSE AND mailbox NOT IN ('Sent', 'Drafts') THEN 1 ELSE 0 END) as unread
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

pub async fn replace_attachments(
    pool: &SqlitePool,
    email_id: i64,
    attachments: &[crate::mail::compose::ComposedAttachment],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM attachments WHERE email_id = ?")
        .bind(email_id)
        .execute(&mut *tx)
        .await?;

    for attachment in attachments {
        sqlx::query(
            "INSERT INTO attachments (email_id, filename, content_type, data, size) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(email_id)
        .bind(&attachment.filename)
        .bind(&attachment.content_type)
        .bind(&attachment.data)
        .bind(attachment.data.len() as i64)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
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

// Outbound queue queries
pub async fn enqueue_outbound_email(
    pool: &SqlitePool,
    envelope_sender: &str,
    recipients: &[String],
    raw_message: &[u8],
) -> anyhow::Result<OutboundQueueItem> {
    let recipients = serde_json::to_string(recipients)?;
    let item = sqlx::query_as::<_, OutboundQueueItem>(
        r#"
        INSERT INTO outbound_queue (envelope_sender, recipients, raw_message, status, next_attempt_at)
        VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)
        RETURNING *
        "#,
    )
    .bind(envelope_sender)
    .bind(recipients)
    .bind(raw_message)
    .bind(OUTBOUND_STATUS_QUEUED)
    .fetch_one(pool)
    .await?;
    Ok(item)
}

pub async fn get_due_outbound_queue_items(
    pool: &SqlitePool,
    limit: i64,
) -> anyhow::Result<Vec<OutboundQueueItem>> {
    let items = sqlx::query_as::<_, OutboundQueueItem>(
        r#"
        SELECT *
        FROM outbound_queue
        WHERE status = ?
          AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
        ORDER BY next_attempt_at ASC, id ASC
        LIMIT ?
        "#,
    )
    .bind(OUTBOUND_STATUS_QUEUED)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

pub async fn list_outbound_queue_items(
    pool: &SqlitePool,
    status: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<OutboundQueueItem>> {
    let limit = limit.clamp(1, 200);
    let items = if let Some(status) = status.filter(|status| !status.is_empty()) {
        sqlx::query_as::<_, OutboundQueueItem>(
            r#"
            SELECT *
            FROM outbound_queue
            WHERE status = ?
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(status)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, OutboundQueueItem>(
            r#"
            SELECT *
            FROM outbound_queue
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };
    Ok(items)
}

pub async fn retry_outbound_queue_item(
    pool: &SqlitePool,
    id: i64,
) -> anyhow::Result<Option<OutboundQueueItem>> {
    let item = sqlx::query_as::<_, OutboundQueueItem>(
        r#"
        UPDATE outbound_queue
        SET status = ?,
            last_error = NULL,
            next_attempt_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ? AND status = ?
        RETURNING *
        "#,
    )
    .bind(OUTBOUND_STATUS_QUEUED)
    .bind(id)
    .bind(OUTBOUND_STATUS_FAILED)
    .fetch_optional(pool)
    .await?;
    Ok(item)
}

pub async fn delete_outbound_queue_item(pool: &SqlitePool, id: i64) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM outbound_queue WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn mark_outbound_sent(pool: &SqlitePool, id: i64) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE outbound_queue
        SET status = ?, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(OUTBOUND_STATUS_SENT)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_outbound_retry(
    pool: &SqlitePool,
    item: &OutboundQueueItem,
    error: &str,
) -> anyhow::Result<()> {
    let next_attempt = chrono::Utc::now().naive_utc() + retry_delay(item.attempts + 1);
    sqlx::query(
        r#"
        UPDATE outbound_queue
        SET attempts = attempts + 1,
            last_error = ?,
            next_attempt_at = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(error)
    .bind(next_attempt)
    .bind(item.id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_outbound_failed(
    pool: &SqlitePool,
    item: &OutboundQueueItem,
    error: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE outbound_queue
        SET attempts = attempts + 1,
            status = ?,
            last_error = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(OUTBOUND_STATUS_FAILED)
    .bind(error)
    .bind(item.id)
    .execute(pool)
    .await?;
    Ok(())
}

pub fn outbound_recipients(item: &OutboundQueueItem) -> Vec<String> {
    serde_json::from_str(&item.recipients).unwrap_or_default()
}

pub fn outbound_should_fail_permanently(item: &OutboundQueueItem) -> bool {
    item.attempts + 1 >= item.max_attempts
}

fn retry_delay(next_attempt_number: i64) -> chrono::Duration {
    let minutes = match next_attempt_number {
        attempt if attempt <= 1 => 5,
        2 => 15,
        3 => 60,
        4 => 6 * 60,
        _ => 24 * 60,
    };
    chrono::Duration::minutes(minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn migrated_db() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        db
    }

    async fn create_test_user(db: &SqlitePool) -> User {
        let domain = create_domain(db, "example.com").await.expect("domain");
        create_user(db, "user@example.com", "hash", domain.id, false)
            .await
            .expect("user")
    }

    async fn save_test_email(db: &SqlitePool, user_id: i64, mailbox: &str) -> Email {
        save_email(
            db,
            NewEmail {
                message_id: Some("<test@example.com>"),
                sender: "sender@example.net",
                recipients: "[\"user@example.com\"]",
                subject: Some("Test message"),
                body_text: Some("Hello"),
                body_html: None,
                raw_message: Some(b"Subject: Test message\r\n\r\nHello"),
                user_id,
                mailbox,
                is_read: matches!(mailbox, "Sent" | "Drafts"),
            },
        )
        .await
        .expect("email")
    }

    #[test]
    fn outbound_retry_delay_increases() {
        assert_eq!(retry_delay(1), chrono::Duration::minutes(5));
        assert_eq!(retry_delay(2), chrono::Duration::minutes(15));
        assert_eq!(retry_delay(3), chrono::Duration::minutes(60));
        assert_eq!(retry_delay(4), chrono::Duration::minutes(360));
        assert_eq!(retry_delay(5), chrono::Duration::minutes(1440));
    }

    #[tokio::test]
    async fn outbound_queue_returns_only_due_queued_items() {
        let db = migrated_db().await;
        let due = enqueue_outbound_email(
            &db,
            "sender@example.com",
            &["a@example.net".to_string()],
            b"message",
        )
        .await
        .expect("due");
        let future = enqueue_outbound_email(
            &db,
            "sender@example.com",
            &["b@example.net".to_string()],
            b"message",
        )
        .await
        .expect("future");
        mark_outbound_retry(&db, &future, "try later")
            .await
            .expect("retry");
        let sent = enqueue_outbound_email(
            &db,
            "sender@example.com",
            &["c@example.net".to_string()],
            b"message",
        )
        .await
        .expect("sent");
        mark_outbound_sent(&db, sent.id).await.expect("mark sent");

        let items = get_due_outbound_queue_items(&db, 10).await.expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, due.id);
    }

    #[tokio::test]
    async fn failed_outbound_item_can_be_retried_and_deleted() {
        let db = migrated_db().await;
        let item = enqueue_outbound_email(
            &db,
            "sender@example.com",
            &["a@example.net".to_string()],
            b"message",
        )
        .await
        .expect("queued");
        mark_outbound_failed(&db, &item, "no route")
            .await
            .expect("failed");

        let failed = list_outbound_queue_items(&db, Some(OUTBOUND_STATUS_FAILED), 10)
            .await
            .expect("failed items");
        assert_eq!(failed.len(), 1);

        let retried = retry_outbound_queue_item(&db, item.id)
            .await
            .expect("retry")
            .expect("item");
        assert_eq!(retried.status, OUTBOUND_STATUS_QUEUED);
        assert_eq!(retried.last_error, None);

        assert!(
            delete_outbound_queue_item(&db, item.id)
                .await
                .expect("delete")
        );
        assert!(
            list_outbound_queue_items(&db, None, 10)
                .await
                .expect("items")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn save_draft_creates_and_updates_single_draft_message() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;

        let created = save_draft(
            &db,
            None,
            DraftEmail {
                sender: "user@example.com",
                recipients: "[\"first@example.net\"]",
                subject: Some("Draft one"),
                body_text: Some("First body"),
                body_html: None,
                raw_message: br#"{"subject":"Draft one"}"#,
                user_id: user.id,
            },
        )
        .await
        .expect("create draft")
        .expect("draft");

        assert_eq!(created.mailbox.as_deref(), Some("Drafts"));
        assert_eq!(created.subject.as_deref(), Some("Draft one"));

        let updated = save_draft(
            &db,
            Some(created.id),
            DraftEmail {
                sender: "user@example.com",
                recipients: "[\"second@example.net\"]",
                subject: Some("Draft two"),
                body_text: Some("Updated body"),
                body_html: None,
                raw_message: br#"{"subject":"Draft two"}"#,
                user_id: user.id,
            },
        )
        .await
        .expect("update draft")
        .expect("draft");

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.subject.as_deref(), Some("Draft two"));
        assert_eq!(updated.body_text.as_deref(), Some("Updated body"));

        let drafts = get_emails_by_user(&db, user.id, "Drafts", 10, 0)
            .await
            .expect("drafts");
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, created.id);
    }

    #[tokio::test]
    async fn replace_attachments_replaces_existing_attachment_set() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let email = save_test_email(&db, user.id, "Drafts").await;

        save_attachment(&db, email.id, Some("old.txt"), Some("text/plain"), b"old")
            .await
            .expect("old attachment");

        replace_attachments(
            &db,
            email.id,
            &[crate::mail::compose::ComposedAttachment {
                filename: "new.txt".to_string(),
                content_type: "text/plain".to_string(),
                data: b"new".to_vec(),
            }],
        )
        .await
        .expect("replace");

        let attachments = get_attachments_by_email(&db, email.id)
            .await
            .expect("attachments");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename.as_deref(), Some("new.txt"));
        assert_eq!(attachments[0].data.as_deref(), Some(b"new".as_slice()));
    }

    #[tokio::test]
    async fn email_summaries_include_attachment_counts() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let email = save_test_email(&db, user.id, "INBOX").await;
        save_attachment(&db, email.id, Some("one.txt"), Some("text/plain"), b"one")
            .await
            .expect("first attachment");
        save_attachment(&db, email.id, Some("two.txt"), Some("text/plain"), b"two")
            .await
            .expect("second attachment");

        let inbox = get_emails_by_user(&db, user.id, "INBOX", 10, 0)
            .await
            .expect("summaries");

        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].id, email.id);
        assert_eq!(inbox[0].attachment_count, 2);
    }

    #[tokio::test]
    async fn search_matches_recipients_html_and_escaped_wildcards() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let percent = save_email(
            &db,
            NewEmail {
                message_id: Some("<percent@example.com>"),
                sender: "sender@example.net",
                recipients: r#"["literal@example.com"]"#,
                subject: Some("Budget 100%"),
                body_text: Some("Literal percent"),
                body_html: Some("<p>HTML needle</p>"),
                raw_message: Some(b"Subject: Budget 100%\r\n\r\nLiteral percent"),
                user_id: user.id,
                mailbox: "INBOX",
                is_read: false,
            },
        )
        .await
        .expect("percent email");
        save_email(
            &db,
            NewEmail {
                message_id: Some("<other@example.com>"),
                sender: "sender@example.net",
                recipients: r#"["special-recipient@example.com"]"#,
                subject: Some("Budget 100X"),
                body_text: Some("Other body"),
                body_html: Some("<p>Other html</p>"),
                raw_message: Some(b"Subject: Budget 100X\r\n\r\nOther body"),
                user_id: user.id,
                mailbox: "INBOX",
                is_read: false,
            },
        )
        .await
        .expect("other email");

        let literal_percent = search_emails(&db, user.id, "%", 10, 0)
            .await
            .expect("literal percent search");
        assert_eq!(literal_percent.len(), 1);
        assert_eq!(literal_percent[0].id, percent.id);
        assert_eq!(
            count_search_emails(&db, user.id, "%")
                .await
                .expect("literal percent count"),
            1
        );

        let recipient = search_emails(&db, user.id, "special-recipient", 10, 0)
            .await
            .expect("recipient search");
        assert_eq!(recipient.len(), 1);
        assert_eq!(
            recipient[0].recipients,
            r#"["special-recipient@example.com"]"#
        );

        let html = search_emails(&db, user.id, "HTML needle", 10, 0)
            .await
            .expect("html search");
        assert_eq!(html.len(), 1);
        assert_eq!(html[0].id, percent.id);
    }

    #[tokio::test]
    async fn save_draft_refuses_to_update_other_users_drafts() {
        let db = migrated_db().await;
        let first = create_test_user(&db).await;
        let domain = get_domain_by_name(&db, "example.com")
            .await
            .expect("domain lookup")
            .expect("domain");
        let second = create_user(&db, "other@example.com", "hash", domain.id, false)
            .await
            .expect("second user");
        let draft = save_draft(
            &db,
            None,
            DraftEmail {
                sender: "user@example.com",
                recipients: "[]",
                subject: Some("Mine"),
                body_text: Some("Mine"),
                body_html: None,
                raw_message: b"{}",
                user_id: first.id,
            },
        )
        .await
        .expect("create draft")
        .expect("draft");

        let updated = save_draft(
            &db,
            Some(draft.id),
            DraftEmail {
                sender: "other@example.com",
                recipients: "[]",
                subject: Some("Not mine"),
                body_text: Some("Nope"),
                body_html: None,
                raw_message: b"{}",
                user_id: second.id,
            },
        )
        .await
        .expect("update attempt");

        assert!(updated.is_none());
        let original = get_email_by_id(&db, draft.id)
            .await
            .expect("load draft")
            .expect("draft");
        assert_eq!(original.user_id, first.id);
        assert_eq!(original.subject.as_deref(), Some("Mine"));
    }

    #[tokio::test]
    async fn email_read_state_can_be_set_both_ways() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let email = save_test_email(&db, user.id, "INBOX").await;

        mark_email_read(&db, email.id).await.expect("mark read");
        let read = get_email_by_id(&db, email.id)
            .await
            .expect("load email")
            .expect("email");
        assert!(read.is_read);

        set_email_read(&db, email.id, false)
            .await
            .expect("mark unread");
        let unread = get_email_by_id(&db, email.id)
            .await
            .expect("load email")
            .expect("email");
        assert!(!unread.is_read);
    }

    #[tokio::test]
    async fn delete_email_moves_visible_message_to_trash() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let email = save_test_email(&db, user.id, "INBOX").await;

        delete_email(&db, email.id).await.expect("delete");

        let updated = get_email_by_id(&db, email.id)
            .await
            .expect("load email")
            .expect("email still exists");
        assert_eq!(updated.mailbox.as_deref(), Some("Trash"));
        assert!(!updated.is_deleted);

        assert!(
            get_emails_by_user(&db, user.id, "INBOX", 10, 0)
                .await
                .expect("inbox emails")
                .is_empty()
        );
        let trash = get_emails_by_user(&db, user.id, "Trash", 10, 0)
            .await
            .expect("trash emails");
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].id, email.id);
    }

    #[tokio::test]
    async fn delete_email_permanently_deletes_trash_message_and_attachments() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let email = save_test_email(&db, user.id, "Trash").await;
        let attachment = save_attachment(
            &db,
            email.id,
            Some("note.txt"),
            Some("text/plain"),
            b"attachment",
        )
        .await
        .expect("attachment");

        delete_email(&db, email.id).await.expect("delete");

        assert!(
            get_email_by_id(&db, email.id)
                .await
                .expect("load email")
                .is_none()
        );
        assert!(
            get_attachment_by_id(&db, attachment.id)
                .await
                .expect("load attachment")
                .is_none()
        );
    }

    #[tokio::test]
    async fn empty_trash_deletes_only_current_users_trash_and_attachments() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let domain = get_domain_by_name(&db, "example.com")
            .await
            .expect("domain query")
            .expect("domain");
        let other = create_user(&db, "other@example.com", "hash", domain.id, false)
            .await
            .expect("other user");

        let trash = save_test_email(&db, user.id, "Trash").await;
        let inbox = save_test_email(&db, user.id, "INBOX").await;
        let other_trash = save_test_email(&db, other.id, "Trash").await;

        let deleted_attachment =
            save_attachment(&db, trash.id, Some("old.txt"), Some("text/plain"), b"old")
                .await
                .expect("deleted attachment");
        let kept_attachment =
            save_attachment(&db, inbox.id, Some("keep.txt"), Some("text/plain"), b"keep")
                .await
                .expect("kept attachment");

        assert_eq!(empty_trash(&db, user.id).await.expect("empty trash"), 1);
        assert!(
            get_email_by_id(&db, trash.id)
                .await
                .expect("load trash")
                .is_none()
        );
        assert!(
            get_attachment_by_id(&db, deleted_attachment.id)
                .await
                .expect("load deleted attachment")
                .is_none()
        );
        assert!(
            get_email_by_id(&db, inbox.id)
                .await
                .expect("load inbox")
                .is_some()
        );
        assert!(
            get_attachment_by_id(&db, kept_attachment.id)
                .await
                .expect("load kept attachment")
                .is_some()
        );
        assert!(
            get_email_by_id(&db, other_trash.id)
                .await
                .expect("load other trash")
                .is_some()
        );
    }

    #[tokio::test]
    async fn permanently_delete_email_removes_any_mailbox_message_and_attachments() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;
        let email = save_test_email(&db, user.id, "Drafts").await;
        let attachment = save_attachment(
            &db,
            email.id,
            Some("draft.txt"),
            Some("text/plain"),
            b"draft data",
        )
        .await
        .expect("attachment");

        assert!(
            permanently_delete_email(&db, email.id)
                .await
                .expect("delete")
        );

        assert!(
            get_email_by_id(&db, email.id)
                .await
                .expect("load email")
                .is_none()
        );
        assert!(
            get_attachment_by_id(&db, attachment.id)
                .await
                .expect("load attachment")
                .is_none()
        );
    }

    #[tokio::test]
    async fn mailbox_counts_ignore_sent_and_drafts_unread_flags() {
        let db = migrated_db().await;
        let user = create_test_user(&db).await;

        let inbox = save_test_email(&db, user.id, "INBOX").await;
        let sent = save_test_email(&db, user.id, "Sent").await;
        let draft = save_test_email(&db, user.id, "Drafts").await;

        set_email_read(&db, sent.id, false)
            .await
            .expect("mark sent unread");
        set_email_read(&db, draft.id, false)
            .await
            .expect("mark draft unread");

        let counts = get_mailbox_counts(&db, user.id)
            .await
            .expect("mailbox counts");
        let find = |name: &str| {
            counts
                .iter()
                .find(|(mailbox, _, _)| mailbox == name)
                .map(|(_, total, unread)| (*total, *unread))
        };

        assert_eq!(find("INBOX"), Some((1, 1)));
        assert_eq!(find("Sent"), Some((1, 0)));
        assert_eq!(find("Drafts"), Some((1, 0)));

        mark_email_read(&db, inbox.id)
            .await
            .expect("mark inbox read");
    }
}
