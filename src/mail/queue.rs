use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::db::models::OutboundQueueItem;
use crate::db::queries;
use crate::smtp::sender::MailSender;

const QUEUE_BATCH_SIZE: i64 = 20;
const QUEUE_TICK_SECONDS: u64 = 30;
const CONCURRENT_SENDS: usize = 5;

pub struct OutboundQueueWorker {
    db: sqlx::SqlitePool,
    sender: MailSender,
    notify_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
}

impl OutboundQueueWorker {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool, notify_rx: tokio::sync::mpsc::UnboundedReceiver<()>) -> Self {
        let sender = MailSender::new(config, db.clone());
        Self { db, sender, notify_rx }
    }

    pub async fn run(mut self) {
        let mut interval = tokio::time::interval(Duration::from_secs(QUEUE_TICK_SECONDS));
        loop {
            tokio::select! {
                _ = self.notify_rx.recv() => {},
                _ = interval.tick() => {},
            }
            if let Err(error) = self.process_due().await {
                error!("Outbound queue worker failed: {}", error);
            }
        }
    }

    pub async fn process_due(&self) -> anyhow::Result<usize> {
        let items = queries::get_due_outbound_queue_items(&self.db, QUEUE_BATCH_SIZE).await?;
        let count = items.len();
        stream::iter(items)
            .map(|item| self.process_item(item))
            .buffer_unordered(CONCURRENT_SENDS)
            .collect::<Vec<_>>()
            .await;
        Ok(count)
    }

    async fn process_item(&self, item: OutboundQueueItem) -> anyhow::Result<()> {
        let recipients = queries::outbound_recipients(&item);
        if recipients.is_empty() {
            queries::mark_outbound_failed(&self.db, &item, "No recipients in queued item").await?;
            return Ok(());
        }

        match self
            .sender
            .send_raw(&item.envelope_sender, &recipients, &item.raw_message)
            .await
        {
            Ok(()) => {
                queries::mark_outbound_sent(&self.db, item.id).await?;
                info!("Outbound queue item {} sent", item.id);
            }
            Err(error) => {
                let error_text = error.to_string();
                if queries::outbound_should_fail_permanently(&item) {
                    queries::mark_outbound_failed(&self.db, &item, &error_text).await?;
                    self.deliver_bounce(&item, &recipients, &error_text).await?;
                    warn!("Outbound queue item {} failed permanently", item.id);
                } else {
                    queries::mark_outbound_retry(&self.db, &item, &error_text).await?;
                    warn!("Outbound queue item {} deferred: {}", item.id, error_text);
                }
            }
        }

        Ok(())
    }

    async fn deliver_bounce(
        &self,
        item: &OutboundQueueItem,
        recipients: &[String],
        error: &str,
    ) -> anyhow::Result<()> {
        if item.envelope_sender.trim().is_empty() {
            return Ok(());
        }

        let Some(user) = queries::get_user_by_email(&self.db, &item.envelope_sender).await? else {
            return Ok(());
        };

        let recipients_json = serde_json::to_string(std::slice::from_ref(&item.envelope_sender))?;
        let subject = "Delivery Status Notification (Failure)";
        let body = format!(
            "Your message could not be delivered.\r\n\r\nRecipients: {}\r\nAttempts: {}\r\nLast error: {}\r\n",
            recipients.join(", "),
            item.attempts + 1,
            error
        );
        let raw = format!(
            "From: MAILER-DAEMON\r\nTo: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}",
            item.envelope_sender, subject, body
        );

        queries::save_email(
            &self.db,
            queries::NewEmail {
                message_id: None,
                sender: "MAILER-DAEMON",
                recipients: &recipients_json,
                subject: Some(subject),
                body_text: Some(&body),
                body_html: None,
                raw_message: Some(raw.as_bytes()),
                user_id: user.id,
                mailbox: "INBOX",
                is_read: false,
            },
        )
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn permanent_failure_creates_bounce_for_local_sender() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        queries::create_user(&db, "sender@example.com", "hash", domain.id, false)
            .await
            .expect("user");
        let item = queries::enqueue_outbound_email(
            &db,
            "sender@example.com",
            &["remote@example.net".to_string()],
            b"From: sender@example.com\r\n\r\nbody",
        )
        .await
        .expect("queued");
        queries::mark_outbound_failed(&db, &item, "network failed")
            .await
            .expect("failed");

        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = OutboundQueueWorker::new(Arc::new(Config::default()), db.clone(), rx);
        worker
            .deliver_bounce(&item, &["remote@example.net".to_string()], "network failed")
            .await
            .expect("bounce");

        let user = queries::get_user_by_email(&db, "sender@example.com")
            .await
            .expect("lookup")
            .expect("user");
        let inbox = queries::get_emails_by_user(&db, user.id, "INBOX", 10, 0)
            .await
            .expect("emails");

        assert_eq!(inbox.len(), 1);
        assert_eq!(
            inbox[0].subject.as_deref(),
            Some("Delivery Status Notification (Failure)")
        );
    }

    #[tokio::test]
    async fn null_reverse_path_does_not_create_bounce() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        queries::create_user(&db, "sender@example.com", "hash", domain.id, false)
            .await
            .expect("user");
        let item = queries::enqueue_outbound_email(
            &db,
            "",
            &["remote@example.net".to_string()],
            b"Subject: bounce\r\n\r\nbody",
        )
        .await
        .expect("queued");

        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = OutboundQueueWorker::new(Arc::new(Config::default()), db.clone(), rx);
        worker
            .deliver_bounce(&item, &["remote@example.net".to_string()], "network failed")
            .await
            .expect("bounce skipped");

        let user = queries::get_user_by_email(&db, "sender@example.com")
            .await
            .expect("lookup")
            .expect("user");
        let inbox = queries::get_emails_by_user(&db, user.id, "INBOX", 10, 0)
            .await
            .expect("emails");

        assert!(inbox.is_empty());
    }
}
