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
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
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
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
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
}
