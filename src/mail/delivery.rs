use std::sync::Arc;
use tracing::{info, warn, error};

use crate::config::Config;
use crate::db::queries;
use crate::mail::parser;
use crate::smtp::sender::MailSender;

pub struct MailDelivery {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    sender: MailSender,
}

impl MailDelivery {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        let sender = MailSender::new(config.clone());
        Self { config, db, sender }
    }

    /// Process an incoming email: parse, store locally, and forward if needed
    pub async fn deliver_incoming(
        &self,
        raw_data: &[u8],
        envelope_sender: &str,
        envelope_recipients: &[String],
    ) -> anyhow::Result<()> {
        let parsed = parser::parse_email(raw_data)?;
        let hostname = &self.config.server.hostname;

        for rcpt in envelope_recipients {
            // Check if this is a local recipient
            if let Some(domain) = rcpt.split('@').last() {
                if domain == hostname || is_local_domain(&self.db, domain).await {
                    // Local delivery
                    if let Some(user) = queries::get_user_by_email(&self.db, rcpt).await? {
                        let recipients_json = serde_json::to_string(envelope_recipients)
                            .unwrap_or_default();

                        let _ = queries::save_email(
                            &self.db,
                            parsed.message_id.as_deref(),
                            envelope_sender,
                            &recipients_json,
                            parsed.subject.as_deref(),
                            parsed.body_text.as_deref(),
                            parsed.body_html.as_deref(),
                            Some(raw_data),
                            user.id,
                            "INBOX",
                        )
                        .await?;

                        info!("Email delivered locally to {}", rcpt);
                    } else {
                        warn!("Local user not found: {}", rcpt);
                    }
                } else {
                    // External delivery - forward
                    info!("Forwarding email to external address: {}", rcpt);
                    if let Err(e) = self
                        .sender
                        .send(
                            envelope_sender,
                            &[rcpt.clone()],
                            parsed.subject.as_deref().unwrap_or("(no subject)"),
                            parsed.body_text.as_deref(),
                            parsed.body_html.as_deref(),
                        )
                        .await
                    {
                        error!("Failed to forward email to {}: {}", rcpt, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Send an email from a local user
    pub async fn send_email(
        &self,
        from: &str,
        to: &[String],
        subject: &str,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> anyhow::Result<()> {
        self.sender.send(from, to, subject, body_text, body_html).await
    }
}

async fn is_local_domain(db: &sqlx::SqlitePool, domain: &str) -> bool {
    queries::get_domain_by_name(db, domain)
        .await
        .ok()
        .flatten()
        .is_some()
}
