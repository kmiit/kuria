use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use lettre::message::{Mailbox, Message, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::KuriaError;

pub struct MailSender {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

impl MailSender {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    /// Send an email to external recipients via SMTP
    pub async fn send(
        &self,
        from: &str,
        to: &[String],
        subject: &str,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> anyhow::Result<()> {
        let from_mailbox: Mailbox = from
            .parse()
            .map_err(|e| KuriaError::Smtp(format!("Invalid from address: {}", e)))?;

        // Group recipients by domain for proper MX resolution
        let mut domain_groups: HashMap<String, Vec<String>> = HashMap::new();
        for rcpt in to {
            let domain = rcpt
                .split('@')
                .next_back()
                .ok_or_else(|| KuriaError::Smtp("Invalid recipient address".to_string()))?
                .to_string();
            domain_groups.entry(domain).or_default().push(rcpt.clone());
        }

        let hostname = crate::config::effective_hostname(&self.config, &self.db).await;
        let mut errors = Vec::new();

        for (domain, recipients) in &domain_groups {
            // Build message with all recipients
            let mut builder = Message::builder()
                .from(from_mailbox.clone())
                .subject(subject);

            for rcpt in recipients {
                let mailbox: Mailbox = rcpt
                    .parse()
                    .map_err(|e| KuriaError::Smtp(format!("Invalid to address: {}", e)))?;
                builder = builder.to(mailbox);
            }

            // Set message ID
            let msg_id = format!(
                "<{}.{}@{}>",
                uuid::Uuid::new_v4(),
                chrono::Utc::now().timestamp(),
                hostname
            );
            builder = builder.message_id(Some(msg_id));

            let message = if let Some(html) = body_html {
                if let Some(text) = body_text {
                    builder
                        .multipart(
                            lettre::message::MultiPart::alternative()
                                .singlepart(
                                    lettre::message::SinglePart::builder()
                                        .header(ContentType::TEXT_PLAIN)
                                        .body(text.to_string()),
                                )
                                .singlepart(
                                    lettre::message::SinglePart::builder()
                                        .header(ContentType::TEXT_HTML)
                                        .body(html.to_string()),
                                ),
                        )
                        .map_err(|e| KuriaError::Smtp(format!("Failed to build message: {}", e)))?
                } else {
                    builder
                        .header(ContentType::TEXT_HTML)
                        .body(html.to_string())
                        .map_err(|e| KuriaError::Smtp(format!("Failed to build message: {}", e)))?
                }
            } else {
                let text = body_text.unwrap_or("");
                builder
                    .header(ContentType::TEXT_PLAIN)
                    .body(text.to_string())
                    .map_err(|e| KuriaError::Smtp(format!("Failed to build message: {}", e)))?
            };

            // Resolve MX for this domain
            let mx_host = self.resolve_mx(domain).await.unwrap_or_else(|| {
                debug!("No MX record for {}, falling back to domain itself", domain);
                domain.clone()
            });

            // Send via SMTP
            match self.send_to_host(&mx_host, message).await {
                Ok(_) => {
                    info!(
                        "Email sent from {} to {:?} via {}",
                        from, recipients, mx_host
                    );
                }
                Err(e) => {
                    warn!("Failed to send to {} via {}: {}", domain, mx_host, e);
                    errors.push(format!("{}: {}", domain, e));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(KuriaError::Smtp(format!(
                "Failed to send to some domains: {}",
                errors.join("; ")
            ))
            .into())
        }
    }

    async fn send_to_host(&self, host: &str, message: lettre::Message) -> anyhow::Result<()> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host.to_string())
            .port(25)
            .build();

        transport
            .send(message)
            .await
            .map_err(|e| KuriaError::Smtp(format!("SMTP delivery to {} failed: {}", host, e)))?;

        Ok(())
    }

    async fn resolve_mx(&self, domain: &str) -> Option<String> {
        let resolver = TokioResolver::builder_tokio().ok()?.build().ok()?;
        let mx_response = resolver.mx_lookup(domain).await.ok()?;
        for record in mx_response.answers() {
            if let RData::MX(mx) = &record.data {
                let host = mx.exchange.to_string();
                let host = host.trim_end_matches('.').to_string();
                return Some(host);
            }
        }
        None
    }
}
