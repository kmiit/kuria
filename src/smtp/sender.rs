use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use lettre::address::{Address, Envelope};
use lettre::message::{
    Attachment as LettreAttachment, Mailbox, Message, MultiPart, SinglePart,
    dkim::{DkimConfig, DkimSigningAlgorithm, DkimSigningKey},
    header::ContentType,
};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::KuriaError;
use crate::mail::compose::ComposedAttachment;

pub struct MailSender {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

pub struct RawComposedMessage<'a> {
    pub from: &'a str,
    pub to: &'a [String],
    pub cc: &'a [String],
    pub envelope_recipients: &'a [String],
    pub subject: &'a str,
    pub body_text: Option<&'a str>,
    pub body_html: Option<&'a str>,
    pub attachments: &'a [ComposedAttachment],
}

impl MailSender {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    pub async fn build_raw_message_with_headers(
        &self,
        message: RawComposedMessage<'_>,
    ) -> anyhow::Result<Vec<u8>> {
        let from_mailbox: Mailbox = message
            .from
            .parse()
            .map_err(|e| KuriaError::Smtp(format!("Invalid from address: {}", e)))?;
        let from_address: Address = message
            .from
            .parse()
            .map_err(|e| KuriaError::Smtp(format!("Invalid from address: {}", e)))?;
        let addresses = message
            .envelope_recipients
            .iter()
            .map(|rcpt| {
                rcpt.parse::<Address>()
                    .map_err(|e| KuriaError::Smtp(format!("Invalid recipient address: {}", e)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let envelope = Envelope::new(Some(from_address), addresses)
            .map_err(|e| KuriaError::Smtp(format!("Invalid envelope: {}", e)))?;
        let hostname = crate::config::effective_hostname(&self.config, &self.db).await;
        let msg_id = format!(
            "<{}.{}@{}>",
            uuid::Uuid::new_v4(),
            chrono::Utc::now().timestamp(),
            hostname
        );

        let mut builder = Message::builder()
            .from(from_mailbox)
            .subject(message.subject)
            .message_id(Some(msg_id))
            .envelope(envelope);

        for rcpt in message.to {
            let mailbox: Mailbox = rcpt
                .parse()
                .map_err(|e| KuriaError::Smtp(format!("Invalid to address: {}", e)))?;
            builder = builder.to(mailbox);
        }
        for rcpt in message.cc {
            let mailbox: Mailbox = rcpt
                .parse()
                .map_err(|e| KuriaError::Smtp(format!("Invalid cc address: {}", e)))?;
            builder = builder.cc(mailbox);
        }

        let mut built_message = build_message_body(
            builder,
            message.body_text,
            message.body_html,
            message.attachments,
        )?;
        self.sign_message_if_configured(message.from, &mut built_message)
            .await;
        Ok(built_message.formatted())
    }

    /// Relay a raw RFC 5322 message to external recipients.
    pub async fn send_raw(
        &self,
        from: &str,
        to: &[String],
        raw_message: &[u8],
    ) -> anyhow::Result<()> {
        let from_address = parse_envelope_sender(from)?;

        let mut domain_groups: HashMap<String, Vec<String>> = HashMap::new();
        for rcpt in to {
            let domain = rcpt
                .split('@')
                .next_back()
                .ok_or_else(|| KuriaError::Smtp("Invalid recipient address".to_string()))?
                .to_string();
            domain_groups.entry(domain).or_default().push(rcpt.clone());
        }

        let mut errors = Vec::new();

        for (domain, recipients) in &domain_groups {
            let addresses = recipients
                .iter()
                .map(|rcpt| {
                    rcpt.parse::<Address>()
                        .map_err(|e| KuriaError::Smtp(format!("Invalid recipient address: {}", e)))
                })
                .collect::<Result<Vec<_>, _>>()?;

            let envelope = Envelope::new(from_address.clone(), addresses)
                .map_err(|e| KuriaError::Smtp(format!("Invalid envelope: {}", e)))?;

            let mx_host = self.resolve_mx(domain).await.unwrap_or_else(|| {
                debug!("No MX record for {}, falling back to domain itself", domain);
                domain.clone()
            });

            match self
                .send_raw_to_host(&mx_host, &envelope, raw_message)
                .await
            {
                Ok(_) => {
                    info!(
                        "Raw email relayed from {} to {:?} via {}",
                        from, recipients, mx_host
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to relay raw email to {} via {}: {}",
                        domain, mx_host, e
                    );
                    errors.push(format!("{}: {}", domain, e));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(KuriaError::Smtp(format!(
                "Failed to relay to some domains: {}",
                errors.join("; ")
            ))
            .into())
        }
    }

    async fn sign_message_if_configured(&self, from: &str, message: &mut Message) {
        let Some(domain_name) = from.split('@').next_back().map(str::to_ascii_lowercase) else {
            return;
        };

        let Ok(Some(domain)) = crate::db::queries::get_domain_by_name(&self.db, &domain_name).await
        else {
            return;
        };

        let Some(private_key) = domain.dkim_private_key.as_deref() else {
            return;
        };
        let Some(selector) = domain.dkim_selector else {
            return;
        };

        let signing_key_pem = match crate::mail::auth::normalize_dkim_private_key_pem(private_key) {
            Ok(signing_key_pem) => signing_key_pem,
            Err(error) => {
                warn!("DKIM key for {} is not usable: {}", domain_name, error);
                return;
            }
        };

        let signing_key = match DkimSigningKey::new(&signing_key_pem, DkimSigningAlgorithm::Rsa) {
            Ok(signing_key) => signing_key,
            Err(error) => {
                warn!(
                    "DKIM key for {} could not be used for signing: {}",
                    domain_name, error
                );
                return;
            }
        };

        message.sign(&DkimConfig::default_config(
            selector,
            domain.domain_name,
            signing_key,
        ));
        debug!("DKIM signed outgoing email for {}", domain_name);
    }

    async fn send_raw_to_host(
        &self,
        host: &str,
        envelope: &Envelope,
        raw_message: &[u8],
    ) -> anyhow::Result<()> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host.to_string())
            .port(25)
            .build();

        transport
            .send_raw(envelope, raw_message)
            .await
            .map_err(|e| KuriaError::Smtp(format!("SMTP relay to {} failed: {}", host, e)))?;

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

fn build_message_body(
    builder: lettre::message::MessageBuilder,
    body_text: Option<&str>,
    body_html: Option<&str>,
    attachments: &[ComposedAttachment],
) -> anyhow::Result<Message> {
    let message = if attachments.is_empty() {
        if let Some(html) = body_html {
            if let Some(text) = body_text {
                builder.multipart(build_alternative_body(text, html))
            } else {
                builder
                    .header(ContentType::TEXT_HTML)
                    .body(html.to_string())
            }
        } else {
            builder
                .header(ContentType::TEXT_PLAIN)
                .body(body_text.unwrap_or("").to_string())
        }
    } else {
        let mut mixed = if let Some(html) = body_html {
            if let Some(text) = body_text {
                MultiPart::mixed().multipart(build_alternative_body(text, html))
            } else {
                MultiPart::mixed().singlepart(html_body_part(html))
            }
        } else {
            MultiPart::mixed().singlepart(text_body_part(body_text.unwrap_or("")))
        };

        for attachment in attachments {
            let content_type = ContentType::parse(&attachment.content_type).unwrap_or_else(|_| {
                ContentType::parse("application/octet-stream")
                    .expect("static fallback content type should parse")
            });
            mixed = mixed.singlepart(
                LettreAttachment::new(attachment.filename.clone())
                    .body(attachment.data.clone(), content_type),
            );
        }

        builder.multipart(mixed)
    }
    .map_err(|e| KuriaError::Smtp(format!("Failed to build message: {}", e)))?;

    Ok(message)
}

fn text_body_part(text: &str) -> SinglePart {
    SinglePart::builder()
        .header(ContentType::TEXT_PLAIN)
        .body(text.to_string())
}

fn html_body_part(html: &str) -> SinglePart {
    SinglePart::builder()
        .header(ContentType::TEXT_HTML)
        .body(html.to_string())
}

fn build_alternative_body(text: &str, html: &str) -> MultiPart {
    MultiPart::alternative()
        .singlepart(text_body_part(text))
        .singlepart(html_body_part(html))
}

fn parse_envelope_sender(from: &str) -> anyhow::Result<Option<Address>> {
    if from.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(from.parse::<Address>().map_err(|e| {
        KuriaError::Smtp(format!("Invalid from address: {}", e))
    })?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_envelope_sender_accepts_null_reverse_path() {
        assert_eq!(parse_envelope_sender("").expect("empty"), None);
        assert_eq!(parse_envelope_sender("   ").expect("blank"), None);
        assert!(
            parse_envelope_sender("sender@example.com")
                .expect("sender")
                .is_some()
        );
        assert!(parse_envelope_sender("bad").is_err());
    }

    #[test]
    fn message_body_with_attachments_is_multipart_mixed() {
        let attachments = vec![ComposedAttachment {
            filename: "note.txt".to_string(),
            content_type: "text/plain".to_string(),
            data: b"attached body".to_vec(),
        }];
        let message = build_message_body(
            Message::builder()
                .from("sender@example.com".parse().expect("from"))
                .to("recipient@example.com".parse().expect("to")),
            Some("Plain body"),
            None,
            &attachments,
        )
        .expect("message");

        let raw = String::from_utf8_lossy(&message.formatted()).to_string();
        assert!(raw.contains("Content-Type: multipart/mixed"));
        assert!(raw.contains("Content-Disposition: attachment; filename=\"note.txt\""));
        assert!(raw.contains("attached body"));
    }
}
