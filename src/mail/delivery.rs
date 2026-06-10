use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

use crate::config::Config;
use crate::db::queries;
use crate::mail::compose::{ComposedAttachment, save_composed_attachments};
use crate::plugin::{PluginManager, mail_delivered_event_json};
use crate::smtp::sender::{MailSender, RawComposedMessage};

static QUEUE_NOTIFIER: once_cell::sync::OnceCell<tokio::sync::mpsc::UnboundedSender<()>> = once_cell::sync::OnceCell::new();

pub fn set_queue_notifier(notifier: tokio::sync::mpsc::UnboundedSender<()>) {
    let _ = QUEUE_NOTIFIER.set(notifier);
}

fn notify_queue() {
    if let Some(notifier) = QUEUE_NOTIFIER.get() {
        let _ = notifier.send(());
    }
}

pub struct MailDelivery {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    sender: MailSender,
    plugins: Option<Arc<PluginManager>>,
}

pub struct ComposedEmail<'a> {
    pub from: &'a str,
    pub to: &'a [String],
    pub cc: &'a [String],
    pub bcc: &'a [String],
    pub subject: &'a str,
    pub body_text: Option<&'a str>,
    pub body_html: Option<&'a str>,
    pub attachments: &'a [ComposedAttachment],
}

impl MailDelivery {
    #[cfg(test)]
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        let sender = MailSender::new(config.clone(), db.clone());
        Self {
            config,
            db,
            sender,
            plugins: None,
        }
    }

    pub fn with_plugins(
        config: Arc<Config>,
        db: sqlx::SqlitePool,
        plugins: Arc<PluginManager>,
    ) -> Self {
        let sender = MailSender::new(config.clone(), db.clone());
        Self {
            config,
            db,
            sender,
            plugins: Some(plugins),
        }
    }

    pub async fn send_composed_email(&self, message: ComposedEmail<'_>) -> anyhow::Result<Vec<u8>> {
        let mut envelope_recipients = Vec::new();
        extend_unique(&mut envelope_recipients, message.to);
        extend_unique(&mut envelope_recipients, message.cc);
        extend_unique(&mut envelope_recipients, message.bcc);

        let (local_recipients, external_recipients) =
            self.split_local_and_external(&envelope_recipients).await?;

        let sent_raw_message = self
            .sender
            .build_raw_message_with_headers(RawComposedMessage {
                from: message.from,
                to: message.to,
                cc: message.cc,
                envelope_recipients: &envelope_recipients,
                subject: message.subject,
                body_text: message.body_text,
                body_html: message.body_html,
                attachments: message.attachments,
            })
            .await?;
        let visible_recipients = visible_recipients_json(message.to, message.cc);
        for local_user in local_recipients {
            let email = queries::save_email(
                &self.db,
                queries::NewEmail {
                    message_id: None,
                    sender: message.from,
                    recipients: &visible_recipients,
                    subject: Some(message.subject),
                    body_text: message.body_text,
                    body_html: message.body_html,
                    raw_message: Some(&sent_raw_message),
                    user_id: local_user.id,
                    mailbox: "INBOX",
                    is_read: false,
                },
            )
            .await?;
            save_composed_attachments(&self.db, email.id, message.attachments).await?;
            if let Some(plugins) = &self.plugins {
                let event_json = mail_delivered_event_json(&email, &local_user.email);
                plugins.call_mail_delivered(&event_json);
            }
            info!(
                "Email delivered locally from {} to {}",
                message.from, local_user.email
            );
        }

        if !external_recipients.is_empty() {
            for recipients in group_recipients_by_domain(&external_recipients)?.into_values() {
                let queued = queries::enqueue_outbound_email(
                    &self.db,
                    message.from,
                    &recipients,
                    &sent_raw_message,
                )
                .await?;
                info!(
                    "Queued outbound email {} from {} to {:?}",
                    queued.id, message.from, recipients
                );
            }
            notify_queue();
        }

        Ok(sent_raw_message)
    }

    pub async fn relay_raw_email(
        &self,
        from: &str,
        to: &[String],
        raw_message: &[u8],
    ) -> anyhow::Result<()> {
        for recipients in group_recipients_by_domain(to)?.into_values() {
            let queued =
                queries::enqueue_outbound_email(&self.db, from, &recipients, raw_message).await?;
            info!(
                "Queued raw outbound email {} from {} to {:?}",
                queued.id, from, recipients
            );
        }
        notify_queue();
        Ok(())
    }

    async fn split_local_and_external(
        &self,
        recipients: &[String],
    ) -> anyhow::Result<(Vec<crate::db::models::User>, Vec<String>)> {
        let hostname = crate::config::effective_hostname(&self.config, &self.db).await;
        let mut local = Vec::new();
        let mut external = Vec::new();

        for recipient in recipients {
            let Some(domain) = recipient.split('@').next_back() else {
                anyhow::bail!("Invalid recipient address: {}", recipient);
            };

            if domain.eq_ignore_ascii_case(&hostname) || is_local_domain(&self.db, domain).await {
                let Some(user) = queries::get_user_by_email(&self.db, recipient).await? else {
                    anyhow::bail!("Local recipient does not exist: {}", recipient);
                };
                local.push(user);
            } else {
                external.push(recipient.clone());
            }
        }

        Ok((local, external))
    }
}

async fn is_local_domain(db: &sqlx::SqlitePool, domain: &str) -> bool {
    queries::get_domain_by_name(db, domain)
        .await
        .ok()
        .flatten()
        .is_some()
}

fn extend_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(value))
        {
            target.push(value.clone());
        }
    }
}

fn visible_recipients_json(to: &[String], cc: &[String]) -> String {
    let mut visible = Vec::new();
    extend_unique(&mut visible, to);
    extend_unique(&mut visible, cc);
    serde_json::to_string(&visible).unwrap_or_default()
}

fn group_recipients_by_domain(
    recipients: &[String],
) -> anyhow::Result<HashMap<String, Vec<String>>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for recipient in recipients {
        let Some(domain) = recipient.split('@').next_back() else {
            anyhow::bail!("Invalid recipient address: {}", recipient);
        };
        groups
            .entry(domain.to_ascii_lowercase())
            .or_default()
            .push(recipient.clone());
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_recipients_exclude_bcc_and_dedupe_case_insensitively() {
        let to = vec!["User@example.com".to_string()];
        let cc = vec!["user@example.com".to_string(), "cc@example.com".to_string()];
        assert_eq!(
            visible_recipients_json(&to, &cc),
            r#"["User@example.com","cc@example.com"]"#
        );
    }

    #[test]
    fn recipients_are_grouped_by_domain_case_insensitively() {
        let groups = group_recipients_by_domain(&[
            "a@example.net".to_string(),
            "b@Example.NET".to_string(),
            "c@example.org".to_string(),
        ])
        .expect("groups");

        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups.get("example.net"),
            Some(&vec![
                "a@example.net".to_string(),
                "b@Example.NET".to_string()
            ])
        );
        assert_eq!(
            groups.get("example.org"),
            Some(&vec!["c@example.org".to_string()])
        );
    }

    #[tokio::test]
    async fn composed_external_email_is_queued() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let delivery = MailDelivery::new(Arc::new(Config::default()), db.clone());
        let to = vec!["remote@example.net".to_string()];

        let sent_raw = delivery
            .send_composed_email(ComposedEmail {
                from: "sender@example.com",
                to: &to,
                cc: &[],
                bcc: &[],
                subject: "Queued",
                body_text: Some("Hello"),
                body_html: None,
                attachments: &[],
            })
            .await
            .expect("send");

        let queued = queries::get_due_outbound_queue_items(&db, 10)
            .await
            .expect("queued");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].envelope_sender, "sender@example.com");
        assert_eq!(
            queries::outbound_recipients(&queued[0]),
            vec!["remote@example.net".to_string()]
        );
        assert!(String::from_utf8_lossy(&queued[0].raw_message).contains("Subject: Queued"));
        assert_eq!(queued[0].raw_message, sent_raw);
    }

    #[tokio::test]
    async fn composed_local_email_is_saved_with_raw_message() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        let user = queries::create_user(&db, "local@example.com", "hash", domain.id, false)
            .await
            .expect("user");
        let delivery = MailDelivery::new(Arc::new(Config::default()), db.clone());
        let to = vec!["local@example.com".to_string()];

        let sent_raw = delivery
            .send_composed_email(ComposedEmail {
                from: "sender@example.com",
                to: &to,
                cc: &[],
                bcc: &[],
                subject: "Local",
                body_text: Some("Hello local"),
                body_html: None,
                attachments: &[],
            })
            .await
            .expect("send");

        let inbox = queries::get_emails_for_imap(&db, user.id, "INBOX")
            .await
            .expect("inbox");
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].raw_message.as_deref(), Some(sent_raw.as_slice()));
        assert!(String::from_utf8_lossy(&sent_raw).contains("Subject: Local"));
    }

    #[tokio::test]
    async fn composed_bcc_only_email_delivers_without_exposing_bcc_header() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        let user = queries::create_user(&db, "local@example.com", "hash", domain.id, false)
            .await
            .expect("user");
        let delivery = MailDelivery::new(Arc::new(Config::default()), db.clone());
        let bcc = vec!["local@example.com".to_string()];

        let sent_raw = delivery
            .send_composed_email(ComposedEmail {
                from: "sender@example.com",
                to: &[],
                cc: &[],
                bcc: &bcc,
                subject: "Bcc only",
                body_text: Some("Hidden recipient"),
                body_html: None,
                attachments: &[],
            })
            .await
            .expect("send");

        let inbox = queries::get_emails_for_imap(&db, user.id, "INBOX")
            .await
            .expect("inbox");
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].recipients, "[]");

        let raw = String::from_utf8_lossy(&sent_raw);
        assert!(raw.contains("Subject: Bcc only"));
        assert!(!raw.contains("Bcc:"));
        assert!(!raw.contains("local@example.com"));
    }

    #[tokio::test]
    async fn composed_local_email_saves_attachment_records() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        let user = queries::create_user(&db, "local@example.com", "hash", domain.id, false)
            .await
            .expect("user");
        let delivery = MailDelivery::new(Arc::new(Config::default()), db.clone());
        let to = vec!["local@example.com".to_string()];
        let attachments = vec![ComposedAttachment {
            filename: "note.txt".to_string(),
            content_type: "text/plain".to_string(),
            data: b"attached body".to_vec(),
        }];

        delivery
            .send_composed_email(ComposedEmail {
                from: "sender@example.com",
                to: &to,
                cc: &[],
                bcc: &[],
                subject: "Attachment",
                body_text: Some("See attachment"),
                body_html: None,
                attachments: &attachments,
            })
            .await
            .expect("send");

        let inbox = queries::get_emails_for_imap(&db, user.id, "INBOX")
            .await
            .expect("inbox");
        let saved_attachments = queries::get_attachments_by_email(&db, inbox[0].id)
            .await
            .expect("attachments");

        assert_eq!(saved_attachments.len(), 1);
        assert_eq!(saved_attachments[0].filename.as_deref(), Some("note.txt"));
        assert_eq!(
            saved_attachments[0].content_type.as_deref(),
            Some("text/plain")
        );
        assert_eq!(
            saved_attachments[0].data.as_deref(),
            Some(b"attached body".as_slice())
        );
    }

    #[tokio::test]
    async fn composed_external_email_queues_one_item_per_domain() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let delivery = MailDelivery::new(Arc::new(Config::default()), db.clone());
        let to = vec![
            "a@example.net".to_string(),
            "b@example.net".to_string(),
            "c@example.org".to_string(),
        ];

        delivery
            .send_composed_email(ComposedEmail {
                from: "sender@example.com",
                to: &to,
                cc: &[],
                bcc: &[],
                subject: "Grouped",
                body_text: Some("Hello"),
                body_html: None,
                attachments: &[],
            })
            .await
            .expect("send");

        let queued = queries::get_due_outbound_queue_items(&db, 10)
            .await
            .expect("queued");
        let mut recipient_sets = queued
            .iter()
            .map(queries::outbound_recipients)
            .collect::<Vec<_>>();
        recipient_sets.sort_by_key(|recipients| recipients.join(","));

        assert_eq!(queued.len(), 2);
        assert_eq!(
            recipient_sets,
            vec![
                vec!["a@example.net".to_string(), "b@example.net".to_string()],
                vec!["c@example.org".to_string()],
            ]
        );
    }

    #[tokio::test]
    async fn raw_relay_queues_one_item_per_domain() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let delivery = MailDelivery::new(Arc::new(Config::default()), db.clone());

        delivery
            .relay_raw_email(
                "sender@example.com",
                &["a@example.net".to_string(), "b@example.org".to_string()],
                b"From: sender@example.com\r\n\r\nHello",
            )
            .await
            .expect("relay");

        let queued = queries::get_due_outbound_queue_items(&db, 10)
            .await
            .expect("queued");
        assert_eq!(queued.len(), 2);
        assert!(
            queued
                .iter()
                .all(|item| queries::outbound_recipients(item).len() == 1)
        );
    }
}
