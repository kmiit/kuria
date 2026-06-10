use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, warn};

use crate::config::Config;
use crate::db::queries;
use crate::imap::mailbox;
use crate::mail::auth::{SpfResult, check_dkim, check_dmarc, check_spf};
use crate::plugin::PluginManager;

const MAX_MAIL_SIZE: usize = 52_428_800; // 50 MB
const MAX_AUTH_FAILURES_PER_SESSION: u32 = 5;

/// Result of a SMTP session
pub enum SmtpSessionResult {
    /// Session ended normally
    Done,
    /// Client requested STARTTLS upgrade
    StartTls,
}

#[derive(Debug, Clone)]
pub struct SmtpSessionOptions {
    pub peer_addr: String,
    pub is_tls: bool,
    pub send_greeting: bool,
    pub starttls_supported: bool,
}

impl SmtpSessionOptions {
    pub fn new(
        peer_addr: impl Into<String>,
        is_tls: bool,
        send_greeting: bool,
        starttls_supported: bool,
    ) -> Self {
        Self {
            peer_addr: peer_addr.into(),
            is_tls,
            send_greeting,
            starttls_supported,
        }
    }
}

/// Full SMTP session handler - works with any async read/write stream
pub async fn handle_smtp_session<R, W>(
    reader: &mut R,
    writer: &mut W,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    plugins: Arc<PluginManager>,
    options: SmtpSessionOptions,
) -> anyhow::Result<SmtpSessionResult>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let SmtpSessionOptions {
        peer_addr,
        is_tls,
        send_greeting,
        starttls_supported,
    } = options;
    let hostname = crate::config::effective_hostname(&config, &db).await;

    if send_greeting {
        let greeting = format!("220 {} ESMTP Kuria Mail Server\r\n", hostname);
        writer.write_all(greeting.as_bytes()).await?;
    }

    // Plugin: on_smtp_connect
    if let Some(result) = plugins.call_smtp_connect(&peer_addr, is_tls)
        && result.reject
    {
        let msg = result
            .reject_message
            .unwrap_or_else(|| "Connection rejected".to_string());
        let resp = format!("554 {}\r\n", msg);
        writer.write_all(resp.as_bytes()).await?;
        return Ok(SmtpSessionResult::Done);
    }

    let mut sender: Option<String> = None;
    let mut recipients: Vec<String> = Vec::new();
    let mut mail_data = Vec::new();
    let mut in_data = false;
    let mut authenticated_user: Option<String> = None;
    let mut auth_failures = 0u32;
    let mut _ehlo_seen = false;
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        debug!("SMTP << {}: {}", peer_addr, trimmed);

        if in_data {
            if trimmed == "." {
                // End of data
                in_data = false;

                // Plugin: on_smtp_data
                let mut plugin_mailbox: Option<String> = None;
                let mut plugin_set_headers: Vec<(String, String)> = Vec::new();
                if let Some(result) = plugins.call_smtp_data(
                    sender.as_deref().unwrap_or(""),
                    &recipients,
                    &mail_data,
                    &peer_addr,
                    is_tls,
                ) {
                    if result.reject {
                        let msg = result
                            .reject_message
                            .unwrap_or_else(|| "Message rejected".to_string());
                        let resp = format!("550 {}\r\n", msg);
                        writer.write_all(resp.as_bytes()).await?;
                        sender = None;
                        recipients.clear();
                        mail_data.clear();
                        continue;
                    }
                    // Apply modifications
                    if let Some(modified) = result.modified_message {
                        mail_data = modified;
                    }
                    plugin_mailbox = result.mailbox;
                    plugin_set_headers = result.set_headers;
                }

                // Check message size
                if mail_data.len() > MAX_MAIL_SIZE {
                    let resp = "552 Message size exceeds maximum\r\n";
                    writer.write_all(resp.as_bytes()).await?;
                    sender = None;
                    recipients.clear();
                    mail_data.clear();
                    continue;
                }

                // Generate message ID
                let msg_id = format!(
                    "<{}.{}@{}>",
                    uuid::Uuid::new_v4(),
                    chrono::Utc::now().timestamp(),
                    hostname
                );

                // Extract sender domain for SPF check
                let sender_domain = sender
                    .as_deref()
                    .and_then(|s| s.split('@').next_back())
                    .unwrap_or("");

                // Perform SPF check
                let spf_ip = peer_ip_for_spf(&peer_addr);
                let spf_result = if sender_domain.is_empty() {
                    SpfResult::None
                } else {
                    check_spf(sender_domain, &spf_ip).await
                };
                let spf_str = spf_result.as_str().to_string();
                debug!(
                    "SPF result for {} from {}: {}",
                    sender_domain, spf_ip, spf_str
                );
                let dkim_result = check_dkim(&mail_data).await;
                let dkim_str = dkim_result.as_str().to_string();
                debug!("DKIM result: {}", dkim_str);

                let message_with_plugin_headers =
                    append_headers_to_header_block(&mail_data, &plugin_set_headers);
                let parsed = crate::mail::parser::parse_email(&message_with_plugin_headers);
                let header_from_domain = parsed
                    .as_ref()
                    .ok()
                    .and_then(|parsed| parsed.from_address.as_deref())
                    .and_then(|from| from.split('@').next_back());
                let dmarc_result = check_dmarc(
                    header_from_domain,
                    non_empty_domain(sender_domain),
                    &spf_result,
                    Some(dkim_str.as_str()),
                )
                .await;
                let dmarc_str = dmarc_result.as_str().to_string();
                let trace_headers = smtp_trace_headers(SmtpTraceContext {
                    hostname: &hostname,
                    peer_addr: &peer_addr,
                    is_tls,
                    spf_result: &spf_str,
                    dkim_result: &dkim_str,
                    dmarc_result: &dmarc_str,
                    envelope_from_domain: sender_domain,
                    header_from_domain,
                });
                let final_mail_data = prepend_headers(&message_with_plugin_headers, &trace_headers);
                let mail_data_str = String::from_utf8_lossy(&message_with_plugin_headers);

                let Some(dest_mailbox) = canonical_smtp_delivery_mailbox(plugin_mailbox.as_deref())
                else {
                    writer
                        .write_all(b"550 5.6.0 Invalid plugin mailbox\r\n")
                        .await?;
                    sender = None;
                    recipients.clear();
                    mail_data.clear();
                    continue;
                };

                let recipient_split =
                    split_local_and_external_recipients(&db, &recipients, &hostname).await;

                if !recipient_split.missing_local.is_empty() {
                    writer
                        .write_all(b"550 5.1.1 Local recipient does not exist\r\n")
                        .await?;
                    sender = None;
                    recipients.clear();
                    mail_data.clear();
                    continue;
                }

                if !recipient_split.external.is_empty() && authenticated_user.is_none() {
                    writer
                        .write_all(b"530 5.7.1 Authentication required for relay\r\n")
                        .await?;
                    sender = None;
                    recipients.clear();
                    mail_data.clear();
                    continue;
                }

                // Store for each local recipient
                let mut delivered = false;
                for rcpt in &recipient_split.local {
                    if let Ok(Some(user)) = queries::get_user_by_email(&db, rcpt).await {
                        let subject = parsed
                            .as_ref()
                            .ok()
                            .and_then(|p| p.subject.as_deref())
                            .or(extract_subject(&mail_data_str));
                        let recipients_json = parsed
                            .as_ref()
                            .ok()
                            .map(|parsed| {
                                serde_json::to_string(&parsed.recipients).unwrap_or_default()
                            })
                            .unwrap_or_else(|| "[]".to_string());

                        let save_result = queries::save_email(
                            &db,
                            queries::NewEmail {
                                message_id: Some(&msg_id),
                                sender: sender.as_deref().unwrap_or(""),
                                recipients: &recipients_json,
                                subject,
                                body_text: parsed
                                    .as_ref()
                                    .ok()
                                    .and_then(|p| p.body_text.as_deref()),
                                body_html: parsed
                                    .as_ref()
                                    .ok()
                                    .and_then(|p| p.body_html.as_deref()),
                                raw_message: Some(&final_mail_data),
                                user_id: user.id,
                                mailbox: dest_mailbox,
                                is_read: false,
                            },
                        )
                        .await;

                        match save_result {
                            Ok(email) => {
                                // Store SPF result
                                let _ = queries::update_email_auth(
                                    &db,
                                    email.id,
                                    Some(spf_str.as_str()),
                                    Some(dkim_str.as_str()),
                                    Some(dmarc_str.as_str()),
                                )
                                .await;

                                // Save attachments
                                if let Ok(ref parsed_email) = parsed {
                                    for att in &parsed_email.attachments {
                                        if !att.data.is_empty() {
                                            if let Err(e) = queries::save_attachment(
                                                &db,
                                                email.id,
                                                att.filename.as_deref(),
                                                att.content_type.as_deref(),
                                                &att.data,
                                            )
                                            .await
                                            {
                                                warn!(
                                                    "Failed to save attachment for email {}: {}",
                                                    email.id, e
                                                );
                                            } else {
                                                debug!(
                                                    "Attachment saved: {:?} ({} bytes)",
                                                    att.filename,
                                                    att.data.len()
                                                );
                                            }
                                        }
                                    }
                                }
                                delivered = true;
                                debug!("Email stored locally for {} (id: {})", rcpt, email.id);
                            }
                            Err(e) => {
                                warn!("Failed to save email for {}: {}", rcpt, e);
                            }
                        }
                    } else {
                        warn!("Local user not found: {}", rcpt);
                    }
                }

                if !recipient_split.external.is_empty() {
                    let delivery =
                        crate::mail::delivery::MailDelivery::new(config.clone(), db.clone());
                    match delivery
                        .relay_raw_email(
                            sender.as_deref().unwrap_or(""),
                            &recipient_split.external,
                            &final_mail_data,
                        )
                        .await
                    {
                        Ok(()) => {
                            delivered = true;
                            debug!(
                                "Relayed email to external recipients: {:?}",
                                recipient_split.external
                            );
                        }
                        Err(e) => {
                            warn!("Failed to relay email: {}", e);
                            writer
                                .write_all(b"451 4.4.0 Relay delivery failed\r\n")
                                .await?;
                            sender = None;
                            recipients.clear();
                            mail_data.clear();
                            continue;
                        }
                    }
                }

                let resp = if delivered {
                    "250 OK: Message accepted for delivery\r\n"
                } else {
                    "250 OK: Message accepted\r\n"
                };
                writer.write_all(resp.as_bytes()).await?;

                // Reset state
                sender = None;
                recipients.clear();
                mail_data.clear();
            } else {
                // Handle dot-stuffing
                if let Some(stripped) = trimmed.strip_prefix('.') {
                    mail_data.extend_from_slice(stripped.as_bytes());
                } else {
                    mail_data.extend_from_slice(trimmed.as_bytes());
                }
                mail_data.extend_from_slice(b"\r\n");
            }
            continue;
        }

        let upper = trimmed.to_uppercase();

        if upper == "QUIT" {
            let resp = format!("221 {} Service closing transmission channel\r\n", hostname);
            writer.write_all(resp.as_bytes()).await?;
            break;
        } else if upper == "NOOP" {
            let resp = "250 OK\r\n";
            writer.write_all(resp.as_bytes()).await?;
        } else if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            _ehlo_seen = true;
            let mut capabilities = format!("250-{}\r\n", hostname);
            for capability in smtp_capabilities(&config, is_tls, starttls_supported) {
                capabilities.push_str("250-");
                capabilities.push_str(capability);
                capabilities.push_str("\r\n");
            }
            capabilities.push_str("250 HELP\r\n");
            writer.write_all(capabilities.as_bytes()).await?;
        } else if upper == "STARTTLS" {
            let starttls_available = config.smtp.enable_starttls && starttls_supported;
            if is_tls {
                let resp = "503 TLS already active\r\n";
                writer.write_all(resp.as_bytes()).await?;
            } else if !starttls_available {
                let resp = "502 STARTTLS not supported\r\n";
                writer.write_all(resp.as_bytes()).await?;
            } else {
                let resp = "220 Ready to start TLS\r\n";
                writer.write_all(resp.as_bytes()).await?;
                writer.flush().await?;
                return Ok(SmtpSessionResult::StartTls);
            }
        } else if upper.starts_with("AUTH ") {
            if smtp_auth_requires_tls(&config, is_tls, starttls_supported) {
                writer
                    .write_all(b"538 5.7.11 Encryption required for requested authentication mechanism\r\n")
                    .await?;
                continue;
            }
            if smtp_auth_locked_out(auth_failures) {
                writer
                    .write_all(b"454 4.7.0 Too many authentication failures\r\n")
                    .await?;
                break;
            }

            let auth_cmd = trimmed[5..].trim();
            let upper_auth = auth_cmd.to_uppercase();

            if upper_auth.starts_with("LOGIN") {
                // AUTH LOGIN - base64 encoded username/password
                let initial_username = auth_cmd
                    .split_once(char::is_whitespace)
                    .map(|(_, initial)| initial.trim())
                    .filter(|initial| !initial.is_empty());
                let username_b64 = if let Some(initial) = initial_username {
                    initial.to_string()
                } else {
                    writer.write_all(b"334 VXNlcm5hbWU6\r\n").await?; // "Username:" in base64
                    let mut username_b64 = String::new();
                    reader.read_line(&mut username_b64).await?;
                    username_b64.trim_end_matches(['\r', '\n']).to_string()
                };

                writer.write_all(b"334 UGFzc3dvcmQ6\r\n").await?; // "Password:" in base64
                let mut password_b64 = String::new();
                reader.read_line(&mut password_b64).await?;
                let password_b64 = password_b64.trim_end_matches(['\r', '\n']);

                // Decode base64
                use base64::Engine;
                use base64::engine::general_purpose::STANDARD;
                let username = STANDARD
                    .decode(username_b64.trim())
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok());
                let password = STANDARD
                    .decode(password_b64)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok());

                match (username, password) {
                    (Some(email), Some(pass)) => {
                        if let Some(auth_email) = authenticate_smtp_user(&db, &email, &pass).await {
                            auth_failures = 0;
                            authenticated_user = Some(auth_email.clone());
                            debug!("SMTP AUTH LOGIN success for {}", auth_email);
                            writer
                                .write_all(b"235 Authentication successful\r\n")
                                .await?;
                        } else {
                            auth_failures = auth_failures.saturating_add(1);
                            warn!("SMTP AUTH LOGIN failed for {}", email);
                            writer.write_all(b"535 Authentication failed\r\n").await?;
                        }
                    }
                    _ => {
                        auth_failures = auth_failures.saturating_add(1);
                        writer.write_all(b"501 Invalid base64 in AUTH\r\n").await?;
                    }
                }
            } else if upper_auth.starts_with("PLAIN") {
                // AUTH PLAIN - may have inline credentials or request them
                let parts: Vec<&str> = auth_cmd.splitn(2, ' ').collect();
                if parts.len() > 1 && !parts[1].is_empty() {
                    // Inline credentials
                    use base64::Engine;
                    use base64::engine::general_purpose::STANDARD;
                    match STANDARD.decode(parts[1].trim()) {
                        Ok(decoded) => {
                            // Format: \0username\0password
                            let parts: Vec<&[u8]> = decoded.split(|&b| b == 0).collect();
                            if parts.len() >= 3 {
                                let email = String::from_utf8_lossy(parts[1]).to_string();
                                let pass = String::from_utf8_lossy(parts[2]).to_string();
                                if let Some(auth_email) =
                                    authenticate_smtp_user(&db, &email, &pass).await
                                {
                                    auth_failures = 0;
                                    authenticated_user = Some(auth_email.clone());
                                    debug!("SMTP AUTH PLAIN success for {}", auth_email);
                                    writer
                                        .write_all(b"235 Authentication successful\r\n")
                                        .await?;
                                } else {
                                    auth_failures = auth_failures.saturating_add(1);
                                    writer.write_all(b"535 Authentication failed\r\n").await?;
                                }
                            } else {
                                auth_failures = auth_failures.saturating_add(1);
                                writer.write_all(b"501 Invalid AUTH PLAIN data\r\n").await?;
                            }
                        }
                        Err(_) => {
                            auth_failures = auth_failures.saturating_add(1);
                            writer.write_all(b"501 Invalid base64 in AUTH\r\n").await?;
                        }
                    }
                } else {
                    // Request credentials
                    writer.write_all(b"334\r\n").await?;
                    let mut cred_b64 = String::new();
                    reader.read_line(&mut cred_b64).await?;
                    let cred_b64 = cred_b64.trim_end_matches(['\r', '\n']);

                    use base64::Engine;
                    use base64::engine::general_purpose::STANDARD;
                    match STANDARD.decode(cred_b64) {
                        Ok(decoded) => {
                            let parts: Vec<&[u8]> = decoded.split(|&b| b == 0).collect();
                            if parts.len() >= 3 {
                                let email = String::from_utf8_lossy(parts[1]).to_string();
                                let pass = String::from_utf8_lossy(parts[2]).to_string();
                                if let Some(auth_email) =
                                    authenticate_smtp_user(&db, &email, &pass).await
                                {
                                    auth_failures = 0;
                                    authenticated_user = Some(auth_email);
                                    writer
                                        .write_all(b"235 Authentication successful\r\n")
                                        .await?;
                                } else {
                                    auth_failures = auth_failures.saturating_add(1);
                                    writer.write_all(b"535 Authentication failed\r\n").await?;
                                }
                            } else {
                                auth_failures = auth_failures.saturating_add(1);
                                writer.write_all(b"501 Invalid AUTH PLAIN data\r\n").await?;
                            }
                        }
                        Err(_) => {
                            auth_failures = auth_failures.saturating_add(1);
                            writer.write_all(b"501 Invalid base64\r\n").await?;
                        }
                    }
                }
            } else {
                let resp = "504 Unsupported AUTH mechanism\r\n";
                writer.write_all(resp.as_bytes()).await?;
            }
        } else if upper.starts_with("MAIL FROM:") {
            let addr = extract_reverse_path(trimmed);
            // Plugin: on_smtp_from
            if let Some(ref s) = addr
                && let Some(result) = plugins.call_smtp_from(s, &peer_addr, is_tls)
                && result.reject
            {
                let msg = result
                    .reject_message
                    .unwrap_or_else(|| "Sender rejected".to_string());
                let resp = format!("550 {}\r\n", msg);
                writer.write_all(resp.as_bytes()).await?;
                continue;
            }
            if let Some(ref sender_addr) = addr {
                if !sender_allowed_for_session(&db, sender_addr, authenticated_user.as_deref())
                    .await
                {
                    writer
                        .write_all(b"553 5.7.1 Sender address not allowed\r\n")
                        .await?;
                    continue;
                }
                sender = Some(sender_addr.clone());
                recipients.clear();
                mail_data.clear();
                let resp = "250 OK\r\n";
                writer.write_all(resp.as_bytes()).await?;
            } else {
                writer
                    .write_all(b"501 5.1.7 Bad sender address syntax\r\n")
                    .await?;
            }
        } else if upper.starts_with("RCPT TO:") {
            if sender.is_none() {
                writer.write_all(b"503 5.5.1 Need MAIL command\r\n").await?;
                continue;
            }
            let addr = extract_address(trimmed);
            // Plugin: on_smtp_to
            if let Some(ref rcpt) = addr
                && let Some(result) =
                    plugins.call_smtp_to(rcpt, sender.as_deref().unwrap_or(""), &peer_addr, is_tls)
                && result.reject
            {
                let msg = result
                    .reject_message
                    .unwrap_or_else(|| "Recipient rejected".to_string());
                let resp = format!("550 {}\r\n", msg);
                writer.write_all(resp.as_bytes()).await?;
                continue;
            }
            if let Some(addr) = addr {
                match classify_recipient_route(&db, &addr, &hostname).await {
                    RecipientRoute::Local => {}
                    RecipientRoute::MissingLocal => {
                        writer.write_all(b"550 5.1.1 User unknown\r\n").await?;
                        continue;
                    }
                    RecipientRoute::External if authenticated_user.is_some() => {}
                    RecipientRoute::External => {
                        writer
                            .write_all(b"554 5.7.1 Relay access denied\r\n")
                            .await?;
                        continue;
                    }
                }
                recipients.push(addr);
            } else {
                writer
                    .write_all(b"501 5.1.3 Bad recipient address syntax\r\n")
                    .await?;
                continue;
            }
            let resp = "250 OK\r\n";
            writer.write_all(resp.as_bytes()).await?;
        } else if upper == "DATA" {
            if sender.is_none() {
                let resp = "503 5.5.1 Need MAIL command\r\n";
                writer.write_all(resp.as_bytes()).await?;
            } else if recipients.is_empty() {
                let resp = "503 5.5.1 No recipients\r\n";
                writer.write_all(resp.as_bytes()).await?;
            } else {
                let resp = "354 Start mail input; end with <CRLF>.<CRLF>\r\n";
                writer.write_all(resp.as_bytes()).await?;
                in_data = true;
                mail_data.clear();
            }
        } else if upper.starts_with("RSET") {
            sender = None;
            recipients.clear();
            mail_data.clear();
            let resp = "250 OK\r\n";
            writer.write_all(resp.as_bytes()).await?;
        } else {
            let resp = "500 5.5.1 Command not recognized\r\n";
            writer.write_all(resp.as_bytes()).await?;
        }
    }

    Ok(SmtpSessionResult::Done)
}

async fn is_local_domain(db: &sqlx::SqlitePool, domain: &str, hostname: &str) -> bool {
    if domain.eq_ignore_ascii_case(hostname) {
        return true;
    }
    queries::get_domain_by_name(db, domain)
        .await
        .ok()
        .flatten()
        .is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecipientRoute {
    Local,
    MissingLocal,
    External,
}

async fn classify_recipient_route(
    db: &sqlx::SqlitePool,
    address: &str,
    hostname: &str,
) -> RecipientRoute {
    let Some(domain) = address.split('@').next_back() else {
        return RecipientRoute::External;
    };
    if !is_local_domain(db, domain, hostname).await {
        return RecipientRoute::External;
    }

    if queries::get_user_by_email(db, address)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        RecipientRoute::Local
    } else {
        RecipientRoute::MissingLocal
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RecipientSplit {
    local: Vec<String>,
    missing_local: Vec<String>,
    external: Vec<String>,
}

async fn split_local_and_external_recipients(
    db: &sqlx::SqlitePool,
    recipients: &[String],
    hostname: &str,
) -> RecipientSplit {
    let mut split = RecipientSplit::default();

    for recipient in recipients {
        match classify_recipient_route(db, recipient, hostname).await {
            RecipientRoute::Local => split.local.push(recipient.clone()),
            RecipientRoute::MissingLocal => split.missing_local.push(recipient.clone()),
            RecipientRoute::External => split.external.push(recipient.clone()),
        }
    }

    split
}

async fn sender_allowed_for_session(
    db: &sqlx::SqlitePool,
    sender: &str,
    authenticated_user: Option<&str>,
) -> bool {
    let Some(user_email) = authenticated_user else {
        return true;
    };

    if sender.eq_ignore_ascii_case(user_email) {
        return true;
    }

    let Some(domain) = sender.split('@').next_back() else {
        return false;
    };

    if queries::get_domain_by_name(db, domain)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return false;
    }

    true
}

async fn authenticate_smtp_user(
    db: &sqlx::SqlitePool,
    email: &str,
    password: &str,
) -> Option<String> {
    let email = normalize_auth_email(email);
    if email.is_empty() {
        return None;
    }

    match queries::get_user_by_email(db, &email).await {
        Ok(Some(user)) if bcrypt::verify(password, &user.password_hash).unwrap_or(false) => {
            Some(email)
        }
        _ => None,
    }
}

fn normalize_auth_email(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn smtp_auth_locked_out(failures: u32) -> bool {
    failures >= MAX_AUTH_FAILURES_PER_SESSION
}

fn smtp_starttls_available(config: &Config, is_tls: bool, starttls_supported: bool) -> bool {
    !is_tls && config.smtp.enable_starttls && starttls_supported
}

fn smtp_auth_requires_tls(config: &Config, is_tls: bool, starttls_supported: bool) -> bool {
    smtp_starttls_available(config, is_tls, starttls_supported)
}

fn smtp_capabilities(config: &Config, is_tls: bool, starttls_supported: bool) -> Vec<&'static str> {
    let mut capabilities = vec!["SIZE 52428800", "8BITMIME"];
    if smtp_starttls_available(config, is_tls, starttls_supported) {
        capabilities.push("STARTTLS");
    } else {
        capabilities.push("AUTH LOGIN PLAIN");
    }
    capabilities
}

fn extract_address(line: &str) -> Option<String> {
    let start = line.find('<')?;
    let end = line.find('>')?;
    if start < end {
        let address = line[start + 1..end].trim().to_ascii_lowercase();
        if is_valid_email_address(&address) {
            Some(address)
        } else {
            None
        }
    } else {
        None
    }
}

fn canonical_smtp_delivery_mailbox(value: Option<&str>) -> Option<&'static str> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(mailbox::INBOX);

    let canonical_mailbox = mailbox::standard_mailboxes()
        .into_iter()
        .find(|standard_mailbox| standard_mailbox.eq_ignore_ascii_case(value))?;

    if matches!(canonical_mailbox, mailbox::SENT | mailbox::DRAFTS) {
        None
    } else {
        Some(canonical_mailbox)
    }
}

fn extract_reverse_path(line: &str) -> Option<String> {
    let start = line.find('<')?;
    let end = line.find('>')?;
    if start < end {
        let address = line[start + 1..end].trim();
        if address.is_empty() {
            return Some(String::new());
        }
    }

    extract_address(line)
}

fn non_empty_domain(domain: &str) -> Option<&str> {
    if domain.is_empty() {
        None
    } else {
        Some(domain)
    }
}

fn peer_ip_for_spf(peer_addr: &str) -> String {
    peer_addr
        .parse::<std::net::SocketAddr>()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| peer_addr.trim().trim_matches(['[', ']']).to_string())
}

struct SmtpTraceContext<'a> {
    hostname: &'a str,
    peer_addr: &'a str,
    is_tls: bool,
    spf_result: &'a str,
    dkim_result: &'a str,
    dmarc_result: &'a str,
    envelope_from_domain: &'a str,
    header_from_domain: Option<&'a str>,
}

fn smtp_trace_headers(context: SmtpTraceContext<'_>) -> Vec<(String, String)> {
    vec![
        (
            "Received".to_string(),
            received_header_value(context.hostname, context.peer_addr, context.is_tls),
        ),
        (
            "Authentication-Results".to_string(),
            authentication_results_header_value(
                context.hostname,
                context.spf_result,
                context.dkim_result,
                context.dmarc_result,
                context.envelope_from_domain,
                context.header_from_domain,
            ),
        ),
    ]
}

fn received_header_value(hostname: &str, peer_addr: &str, is_tls: bool) -> String {
    let protocol = if is_tls { "ESMTPS" } else { "ESMTP" };
    format!(
        "from {} by {} with {}; {}",
        received_peer_identity(peer_addr),
        sanitize_header_value(hostname),
        protocol,
        chrono::Utc::now().to_rfc2822()
    )
}

fn received_peer_identity(peer_addr: &str) -> String {
    let peer_ip = peer_ip_for_spf(peer_addr);
    match peer_ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => format!("[{}]", ip),
        Ok(std::net::IpAddr::V6(ip)) => format!("[IPv6:{}]", ip),
        Err(_) => sanitize_header_value(&peer_ip),
    }
}

fn authentication_results_header_value(
    hostname: &str,
    spf_result: &str,
    dkim_result: &str,
    dmarc_result: &str,
    envelope_from_domain: &str,
    header_from_domain: Option<&str>,
) -> String {
    let smtp_mailfrom = if envelope_from_domain.is_empty() {
        "<>".to_string()
    } else {
        sanitize_header_value(envelope_from_domain)
    };
    let header_from = header_from_domain
        .map(sanitize_header_value)
        .filter(|domain| !domain.is_empty())
        .map(|domain| format!(" header.from={}", domain))
        .unwrap_or_default();

    format!(
        "{}; spf={} smtp.mailfrom={}; dkim={}; dmarc={}{}",
        sanitize_header_value(hostname),
        sanitize_header_value(spf_result),
        smtp_mailfrom,
        sanitize_header_value(dkim_result),
        sanitize_header_value(dmarc_result),
        header_from
    )
}

fn prepend_headers(raw_message: &[u8], headers: &[(String, String)]) -> Vec<u8> {
    let rendered_headers = render_headers(headers);

    if rendered_headers.is_empty() {
        return raw_message.to_vec();
    }

    let mut combined = Vec::with_capacity(rendered_headers.len() + raw_message.len());
    combined.extend_from_slice(&rendered_headers);
    combined.extend_from_slice(raw_message);
    combined
}

fn append_headers_to_header_block(raw_message: &[u8], headers: &[(String, String)]) -> Vec<u8> {
    let rendered_headers = render_headers(headers);

    if rendered_headers.is_empty() {
        return raw_message.to_vec();
    }

    if let Some(body_start) = find_header_body_separator(raw_message) {
        let mut combined = Vec::with_capacity(rendered_headers.len() + raw_message.len());
        combined.extend_from_slice(&raw_message[..body_start.header_end]);
        combined.extend_from_slice(&rendered_headers);
        combined.extend_from_slice(&raw_message[body_start.header_end..]);
        return combined;
    }

    let mut combined = Vec::with_capacity(rendered_headers.len() + raw_message.len());
    combined.extend_from_slice(&rendered_headers);
    combined.extend_from_slice(raw_message);
    combined
}

fn render_headers(headers: &[(String, String)]) -> Vec<u8> {
    let mut rendered_headers = Vec::new();
    for (name, value) in headers {
        let Some(name) = sanitize_header_name(name) else {
            continue;
        };
        rendered_headers.extend_from_slice(name.as_bytes());
        rendered_headers.extend_from_slice(b": ");
        rendered_headers.extend_from_slice(sanitize_header_value(value).as_bytes());
        rendered_headers.extend_from_slice(b"\r\n");
    }
    rendered_headers
}

struct HeaderBodySeparator {
    header_end: usize,
}

fn find_header_body_separator(raw_message: &[u8]) -> Option<HeaderBodySeparator> {
    raw_message
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| HeaderBodySeparator {
            header_end: index + 2,
        })
        .or_else(|| {
            raw_message
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| HeaderBodySeparator {
                    header_end: index + 1,
                })
        })
}

fn sanitize_header_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || !name.bytes().all(|byte| matches!(byte, 33..=57 | 59..=126)) {
        return None;
    }
    Some(name.to_string())
}

fn sanitize_header_value(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '\r' || ch == '\n' || ch == '\0' || (ch.is_control() && ch != '\t') {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_valid_email_address(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };

    !local.is_empty()
        && local.len() <= 64
        && !local.contains(char::is_whitespace)
        && domain.contains('.')
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

fn extract_subject(data: &str) -> Option<&str> {
    for line in data.lines() {
        if line.is_empty() {
            break;
        }
        if line.to_lowercase().starts_with("subject:") {
            return Some(line[8..].trim());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_and_normalizes_smtp_addresses() {
        assert_eq!(
            extract_address("MAIL FROM:<User@Example.COM>"),
            Some("user@example.com".to_string())
        );
        assert_eq!(extract_address("RCPT TO:<bad>"), None);
        assert_eq!(extract_address("MAIL FROM:<>"), None);
        assert_eq!(extract_reverse_path("MAIL FROM:<>"), Some(String::new()));
        assert_eq!(
            extract_reverse_path("MAIL FROM:<User@Example.COM>"),
            Some("user@example.com".to_string())
        );
    }

    #[test]
    fn plugin_mailbox_overrides_are_limited_to_delivery_mailboxes() {
        assert_eq!(canonical_smtp_delivery_mailbox(None), Some(mailbox::INBOX));
        assert_eq!(
            canonical_smtp_delivery_mailbox(Some(" spam ")),
            Some(mailbox::SPAM)
        );
        assert_eq!(
            canonical_smtp_delivery_mailbox(Some("Trash")),
            Some(mailbox::TRASH)
        );
        assert_eq!(canonical_smtp_delivery_mailbox(Some("Drafts")), None);
        assert_eq!(canonical_smtp_delivery_mailbox(Some("Sent")), None);
        assert_eq!(canonical_smtp_delivery_mailbox(Some("../INBOX")), None);
    }

    #[test]
    fn peer_ip_for_spf_removes_socket_port() {
        assert_eq!(peer_ip_for_spf("203.0.113.4:2525"), "203.0.113.4");
        assert_eq!(peer_ip_for_spf("[2001:db8::1]:2525"), "2001:db8::1");
        assert_eq!(peer_ip_for_spf("2001:db8::1"), "2001:db8::1");
    }

    #[test]
    fn prepended_headers_are_added_before_original_message() {
        let raw = b"From: sender@example.com\r\nSubject: Hello\r\n\r\nBody";
        let updated = prepend_headers(
            raw,
            &[
                ("Received".to_string(), "from [203.0.113.4]".to_string()),
                (
                    "Authentication-Results".to_string(),
                    "mail.example.com; spf=pass".to_string(),
                ),
            ],
        );

        assert_eq!(
            String::from_utf8(updated).expect("utf8"),
            "Received: from [203.0.113.4]\r\nAuthentication-Results: mail.example.com; spf=pass\r\nFrom: sender@example.com\r\nSubject: Hello\r\n\r\nBody"
        );
    }

    #[test]
    fn plugin_headers_are_added_to_existing_header_block() {
        let updated = append_headers_to_header_block(
            b"Subject: Hello\r\n\r\nBody",
            &[("X-Kuria-Plugin".to_string(), "checked".to_string())],
        );

        assert_eq!(
            String::from_utf8(updated).expect("utf8"),
            "Subject: Hello\r\nX-Kuria-Plugin: checked\r\n\r\nBody"
        );
    }

    #[test]
    fn added_headers_reject_bad_names_and_sanitize_values() {
        let updated = append_headers_to_header_block(
            b"Subject: Hello\r\n\r\nBody",
            &[
                ("Bad\r\nInjected".to_string(), "ignored".to_string()),
                (
                    "X-Kuria-Plugin".to_string(),
                    "ok\r\nX-Injected: bad\0tail".to_string(),
                ),
            ],
        );
        let updated = String::from_utf8(updated).expect("utf8");

        assert!(!updated.contains("Bad"));
        assert_eq!(
            updated,
            "Subject: Hello\r\nX-Kuria-Plugin: ok X-Injected: bad tail\r\n\r\nBody"
        );
    }

    #[test]
    fn smtp_trace_headers_include_received_and_authentication_results() {
        let headers = smtp_trace_headers(SmtpTraceContext {
            hostname: "mail.example.com",
            peer_addr: "203.0.113.4:2525",
            is_tls: true,
            spf_result: "pass",
            dkim_result: "none",
            dmarc_result: "pass",
            envelope_from_domain: "example.com",
            header_from_domain: Some("example.com"),
        });

        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].0, "Received");
        assert!(headers[0].1.contains("from [203.0.113.4]"));
        assert!(headers[0].1.contains("with ESMTPS"));
        assert_eq!(headers[1].0, "Authentication-Results");
        assert!(headers[1].1.contains("mail.example.com; spf=pass"));
        assert!(headers[1].1.contains("dkim=none"));
        assert!(headers[1].1.contains("dmarc=pass"));
        assert!(headers[1].1.contains("smtp.mailfrom=example.com"));
        assert!(headers[1].1.contains("header.from=example.com"));
    }

    #[test]
    fn smtp_auth_lockout_starts_after_session_failure_limit() {
        assert!(!smtp_auth_locked_out(MAX_AUTH_FAILURES_PER_SESSION - 1));
        assert!(smtp_auth_locked_out(MAX_AUTH_FAILURES_PER_SESSION));
    }

    #[test]
    fn auth_email_is_normalized() {
        assert_eq!(
            normalize_auth_email(" User@Example.COM "),
            "user@example.com"
        );
    }

    fn tls_smtp_options() -> SmtpSessionOptions {
        SmtpSessionOptions::new("127.0.0.1:2525", true, false, false)
    }

    #[tokio::test]
    async fn smtp_auth_accepts_normalized_email() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        let hash = bcrypt::hash("pass", 4).expect("hash");
        queries::create_user(&db, "user@example.com", &hash, domain.id, false)
            .await
            .expect("user");

        assert_eq!(
            authenticate_smtp_user(&db, " User@Example.COM ", "pass").await,
            Some("user@example.com".to_string())
        );
        assert_eq!(
            authenticate_smtp_user(&db, " User@Example.COM ", "wrong").await,
            None
        );
    }

    #[tokio::test]
    async fn rcpt_requires_mail_transaction() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let plugins = Arc::new(PluginManager::load(&Config::default()).expect("plugins"));
        let mut input =
            tokio::io::BufReader::new(b"RCPT TO:<user@example.com>\r\nQUIT\r\n".as_slice());
        let mut output = Vec::new();

        handle_smtp_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db,
            plugins,
            tls_smtp_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("503 5.5.1 Need MAIL command"));
    }

    #[tokio::test]
    async fn empty_reverse_path_is_accepted_for_bounces() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        let user = queries::create_user(&db, "user@example.com", "hash", domain.id, false)
            .await
            .expect("user");
        let plugins = Arc::new(PluginManager::load(&Config::default()).expect("plugins"));
        let mut input = tokio::io::BufReader::new(
            b"MAIL FROM:<>\r\nRCPT TO:<user@example.com>\r\nDATA\r\nSubject: Delivery status\r\n\r\nFailed\r\n.\r\nQUIT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();

        handle_smtp_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db.clone(),
            plugins,
            tls_smtp_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");
        let saved = queries::get_emails_for_imap(&db, user.id, "INBOX")
            .await
            .expect("emails");

        assert!(output.contains("250 OK: Message accepted for delivery"));
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].sender, "");
        let raw = String::from_utf8_lossy(saved[0].raw_message.as_deref().unwrap_or_default());
        assert!(raw.starts_with("Received:"));
        assert!(raw.contains("\r\nAuthentication-Results:"));
    }

    #[tokio::test]
    async fn smtp_delivery_stores_visible_header_recipients_not_envelope_recipients() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        let user = queries::create_user(&db, "hidden@example.com", "hash", domain.id, false)
            .await
            .expect("user");
        let plugins = Arc::new(PluginManager::load(&Config::default()).expect("plugins"));
        let mut input = tokio::io::BufReader::new(
            b"MAIL FROM:<sender@example.net>\r\nRCPT TO:<hidden@example.com>\r\nDATA\r\nFrom: sender@example.net\r\nTo: visible@example.net\r\nCc: copy@example.net\r\nSubject: Header recipients\r\n\r\nBody\r\n.\r\nQUIT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();

        handle_smtp_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db.clone(),
            plugins,
            tls_smtp_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");
        let saved = queries::get_emails_for_imap(&db, user.id, "INBOX")
            .await
            .expect("emails");

        assert!(output.contains("250 OK: Message accepted for delivery"));
        assert_eq!(saved.len(), 1);
        assert_eq!(
            saved[0].recipients,
            r#"["visible@example.net","copy@example.net"]"#
        );
        assert!(!saved[0].recipients.contains("hidden@example.com"));
    }

    #[test]
    fn smtp_auth_requires_tls_only_when_starttls_is_available() {
        let mut config = Config::default();
        config.smtp.enable_starttls = true;

        assert!(smtp_auth_requires_tls(&config, false, true));
        assert!(!smtp_auth_requires_tls(&config, true, true));
        assert!(!smtp_auth_requires_tls(&config, false, false));

        config.smtp.enable_starttls = false;
        assert!(!smtp_auth_requires_tls(&config, false, true));
    }

    #[test]
    fn smtp_capabilities_hide_auth_until_starttls() {
        let mut config = Config::default();
        config.smtp.enable_starttls = true;

        let plain = smtp_capabilities(&config, false, true);
        assert!(plain.contains(&"STARTTLS"));
        assert!(!plain.contains(&"AUTH LOGIN PLAIN"));

        let tls = smtp_capabilities(&config, true, false);
        assert!(tls.contains(&"AUTH LOGIN PLAIN"));
        assert!(!tls.contains(&"STARTTLS"));
    }

    #[tokio::test]
    async fn recipient_split_keeps_missing_local_users_out_of_external_relay() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        crate::db::run_migrations(&db).await.expect("migrations");
        let domain = queries::create_domain(&db, "example.com")
            .await
            .expect("domain");
        queries::create_user(&db, "user@example.com", "hash", domain.id, false)
            .await
            .expect("user");

        let split = split_local_and_external_recipients(
            &db,
            &[
                "user@example.com".to_string(),
                "missing@example.com".to_string(),
                "friend@example.net".to_string(),
            ],
            "mail.example.com",
        )
        .await;

        assert_eq!(split.local, vec!["user@example.com"]);
        assert_eq!(split.missing_local, vec!["missing@example.com"]);
        assert_eq!(split.external, vec!["friend@example.net"]);
    }
}
