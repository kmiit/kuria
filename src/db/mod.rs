pub mod models;
pub mod queries;

use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

pub async fn init_pool(database_url: &str) -> anyhow::Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS domains (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            domain_name TEXT NOT NULL UNIQUE,
            dkim_private_key TEXT,
            dkim_public_key TEXT,
            dkim_selector TEXT DEFAULT 'kuria',
            spf_record TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        UPDATE domains
        SET spf_record = 'v=spf1 mx:' || domain_name || ' -all'
        WHERE spf_record IS NULL OR spf_record = '';
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            domain_id INTEGER NOT NULL,
            is_admin BOOLEAN DEFAULT FALSE,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (domain_id) REFERENCES domains(id) ON DELETE RESTRICT
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS emails (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message_id TEXT,
            sender TEXT NOT NULL,
            recipients TEXT NOT NULL,
            subject TEXT,
            body_text TEXT,
            body_html TEXT,
            raw_message BLOB,
            dkim_signature TEXT,
            spf_result TEXT,
            dmarc_result TEXT,
            is_read BOOLEAN DEFAULT FALSE,
            is_deleted BOOLEAN DEFAULT FALSE,
            mailbox TEXT DEFAULT 'INBOX',
            user_id INTEGER NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS attachments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email_id INTEGER NOT NULL,
            filename TEXT,
            content_type TEXT,
            data BLOB,
            size INTEGER,
            FOREIGN KEY (email_id) REFERENCES emails(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_users_domain_id ON users(domain_id)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_emails_user_mailbox_created ON emails(user_id, mailbox, created_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_attachments_email_id ON attachments(email_id)")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS outbound_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            envelope_sender TEXT NOT NULL,
            recipients TEXT NOT NULL,
            raw_message BLOB NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            max_attempts INTEGER NOT NULL DEFAULT 5,
            status TEXT NOT NULL DEFAULT 'queued',
            last_error TEXT,
            next_attempt_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_outbound_queue_status_next_attempt ON outbound_queue(status, next_attempt_at)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS system_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;

    normalize_stored_dkim_keys(pool).await?;
    tracing::info!("Database migrations completed");
    Ok(())
}

async fn normalize_stored_dkim_keys(pool: &SqlitePool) -> anyhow::Result<()> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, dkim_private_key FROM domains WHERE dkim_private_key IS NOT NULL AND dkim_private_key != ''",
    )
    .fetch_all(pool)
    .await?;

    for (domain_id, private_key) in rows {
        match crate::mail::auth::normalize_dkim_private_key_pem(&private_key) {
            Ok(normalized) if normalized != private_key => {
                sqlx::query("UPDATE domains SET dkim_private_key = ? WHERE id = ?")
                    .bind(normalized)
                    .bind(domain_id)
                    .execute(pool)
                    .await?;
                tracing::info!(
                    "Normalized DKIM private key format for domain id {}",
                    domain_id
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    "Stored DKIM private key for domain id {} is not usable: {}",
                    domain_id,
                    error
                );
            }
        }
    }

    Ok(())
}
