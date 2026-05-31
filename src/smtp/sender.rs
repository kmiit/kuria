use std::sync::Arc;
use lettre::message::{header::ContentType, Mailbox, Message};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use tracing::{info, debug};

use crate::config::Config;
use crate::error::KuriaError;

pub struct MailSender {
    config: Arc<Config>,
}

impl MailSender {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
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
        let from_mailbox: Mailbox = from.parse()
            .map_err(|e| KuriaError::Smtp(format!("Invalid from address: {}", e)))?;

        let mut builder = Message::builder()
            .from(from_mailbox)
            .subject(subject);

        for rcpt in to {
            let mailbox: Mailbox = rcpt.parse()
                .map_err(|e| KuriaError::Smtp(format!("Invalid to address: {}", e)))?;
            builder = builder.to(mailbox);
        }

        // Set message ID
        let msg_id = format!(
            "<{}.{}@{}>",
            uuid::Uuid::new_v4(),
            chrono::Utc::now().timestamp(),
            self.config.server.hostname
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

        // Resolve MX records for the first recipient's domain
        let domain = to[0]
            .split('@')
            .last()
            .ok_or_else(|| KuriaError::Smtp("Invalid recipient address".to_string()))?;

        let mx_host = self.resolve_mx(domain).await.unwrap_or_else(|| {
            debug!("No MX record for {}, falling back to domain itself", domain);
            domain.to_string()
        });

        // Send via SMTP
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(mx_host)
            .port(25)
            .build();

        transport
            .send(message)
            .await
            .map_err(|e| KuriaError::Smtp(format!("Failed to send email: {}", e)))?;

        info!("Email sent from {} to {:?}", from, to);
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
