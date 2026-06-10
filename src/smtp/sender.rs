use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use lettre::address::{Address, Envelope};
use lettre::message::{
    Mailbox, Message,
    dkim::{DkimConfig, DkimSigningAlgorithm, DkimSigningKey},
    header::ContentType,
};
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

pub struct RawComposedMessage<'a> {
    pub from: &'a str,
    pub to: &'a [String],
    pub cc: &'a [String],
    pub envelope_recipients: &'a [String],
    pub subject: &'a str,
    pub body_text: Option<&'a str>,
    pub body_html: Option<&'a str>,
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

        let mut built_message = build_message_body(builder, message.body_text, message.body_html)?;
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
) -> anyhow::Result<Message> {
    let message = if let Some(html) = body_html {
        if let Some(text) = body_text {
            builder.multipart(
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
    .map_err(|e| KuriaError::Smtp(format!("Failed to build message: {}", e)))?;

    Ok(message)
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
}
