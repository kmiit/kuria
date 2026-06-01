use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, warn};

use crate::config::Config;
use crate::db::queries;
use crate::mail::auth::check_spf;

const MAX_MAIL_SIZE: usize = 52_428_800; // 50 MB

/// Result of a SMTP session
pub enum SmtpSessionResult {
    /// Session ended normally
    Done,
    /// Client requested STARTTLS upgrade
    StartTls,
}

/// Full SMTP session handler - works with any async read/write stream
pub async fn handle_smtp_session<R, W>(
    mut reader: R,
    mut writer: W,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    peer_addr: String,
    _is_tls: bool,
) -> anyhow::Result<SmtpSessionResult>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let hostname = config.server.hostname.clone();

    // Send greeting
    let greeting = format!("220 {} ESMTP Kuria Mail Server\r\n", hostname);
    writer.write_all(greeting.as_bytes()).await?;

    let mut sender: Option<String> = None;
    let mut recipients: Vec<String> = Vec::new();
    let mut mail_data = Vec::new();
    let mut in_data = false;
    let mut authenticated = !_is_tls; // TLS connections don't require AUTH
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
                let mail_data_str = String::from_utf8_lossy(&mail_data);

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
                let spf_result = check_spf(sender_domain, &peer_addr).await;
                let spf_str = spf_result.as_str().to_string();
                debug!(
                    "SPF result for {} from {}: {}",
                    sender_domain, peer_addr, spf_str
                );

                // Store for each local recipient
                let mut delivered = false;
                for rcpt in &recipients {
                    if let Some(domain) = rcpt.split('@').next_back()
                        && is_local_domain(&db, domain, &hostname).await
                    {
                        if let Ok(Some(user)) = queries::get_user_by_email(&db, rcpt).await {
                            // Parse with mail_parser for proper subject extraction
                            let parsed = crate::mail::parser::parse_email(&mail_data);
                            let subject = parsed
                                .as_ref()
                                .ok()
                                .and_then(|p| p.subject.as_deref())
                                .or(extract_subject(&mail_data_str));

                            let save_result = queries::save_email(
                                &db,
                                Some(&msg_id),
                                sender.as_deref().unwrap_or(""),
                                &serde_json::to_string(&recipients).unwrap_or_default(),
                                subject,
                                parsed.as_ref().ok().and_then(|p| p.body_text.as_deref()),
                                parsed.as_ref().ok().and_then(|p| p.body_html.as_deref()),
                                Some(&mail_data),
                                user.id,
                                "INBOX",
                            )
                            .await;

                            match save_result {
                                Ok(email) => {
                                    // Store SPF result
                                    let _ = queries::update_email_auth(
                                        &db,
                                        email.id,
                                        Some(spf_str.as_str()),
                                        None,
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
            capabilities.push_str("250-SIZE 52428800\r\n");
            capabilities.push_str("250-8BITMIME\r\n");
            capabilities.push_str("250-AUTH LOGIN PLAIN\r\n");
            let starttls_available = !_is_tls
                && config.smtp.enable_starttls
                && config.tls.cert_path.exists()
                && config.tls.key_path.exists();
            if starttls_available {
                capabilities.push_str("250-STARTTLS\r\n");
            }
            capabilities.push_str("250 HELP\r\n");
            writer.write_all(capabilities.as_bytes()).await?;
        } else if upper == "STARTTLS" {
            let starttls_available = config.smtp.enable_starttls
                && config.tls.cert_path.exists()
                && config.tls.key_path.exists();
            if _is_tls {
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
            let auth_cmd = trimmed[5..].trim();
            let upper_auth = auth_cmd.to_uppercase();

            if upper_auth.starts_with("LOGIN") {
                // AUTH LOGIN - base64 encoded username/password
                writer.write_all(b"334 VXNlcm5hbWU6\r\n").await?; // "Username:" in base64
                let mut username_b64 = String::new();
                reader.read_line(&mut username_b64).await?;
                let username_b64 = username_b64.trim_end_matches(['\r', '\n']);

                writer.write_all(b"334 UGFzc3dvcmQ6\r\n").await?; // "Password:" in base64
                let mut password_b64 = String::new();
                reader.read_line(&mut password_b64).await?;
                let password_b64 = password_b64.trim_end_matches(['\r', '\n']);

                // Decode base64
                use base64::Engine;
                use base64::engine::general_purpose::STANDARD;
                let username = STANDARD
                    .decode(username_b64)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok());
                let password = STANDARD
                    .decode(password_b64)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok());

                match (username, password) {
                    (Some(email), Some(pass)) => {
                        match queries::get_user_by_email(&db, &email).await {
                            Ok(Some(user)) => {
                                if bcrypt::verify(&pass, &user.password_hash).unwrap_or(false) {
                                    authenticated = true;
                                    debug!("SMTP AUTH LOGIN success for {}", email);
                                    writer
                                        .write_all(b"235 Authentication successful\r\n")
                                        .await?;
                                } else {
                                    warn!("SMTP AUTH LOGIN failed for {}: bad password", email);
                                    writer.write_all(b"535 Authentication failed\r\n").await?;
                                }
                            }
                            _ => {
                                warn!("SMTP AUTH LOGIN failed: user not found {}", email);
                                writer.write_all(b"535 Authentication failed\r\n").await?;
                            }
                        }
                    }
                    _ => {
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
                                match queries::get_user_by_email(&db, &email).await {
                                    Ok(Some(user))
                                        if bcrypt::verify(&pass, &user.password_hash)
                                            .unwrap_or(false) =>
                                    {
                                        authenticated = true;
                                        debug!("SMTP AUTH PLAIN success for {}", email);
                                        writer
                                            .write_all(b"235 Authentication successful\r\n")
                                            .await?;
                                    }
                                    _ => {
                                        writer.write_all(b"535 Authentication failed\r\n").await?;
                                    }
                                }
                            } else {
                                writer.write_all(b"501 Invalid AUTH PLAIN data\r\n").await?;
                            }
                        }
                        Err(_) => {
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
                                match queries::get_user_by_email(&db, &email).await {
                                    Ok(Some(user))
                                        if bcrypt::verify(&pass, &user.password_hash)
                                            .unwrap_or(false) =>
                                    {
                                        authenticated = true;
                                        writer
                                            .write_all(b"235 Authentication successful\r\n")
                                            .await?;
                                    }
                                    _ => {
                                        writer.write_all(b"535 Authentication failed\r\n").await?;
                                    }
                                }
                            } else {
                                writer.write_all(b"501 Invalid AUTH PLAIN data\r\n").await?;
                            }
                        }
                        Err(_) => {
                            writer.write_all(b"501 Invalid base64\r\n").await?;
                        }
                    }
                }
            } else {
                let resp = "504 Unsupported AUTH mechanism\r\n";
                writer.write_all(resp.as_bytes()).await?;
            }
        } else if upper.starts_with("MAIL FROM:") {
            let addr = extract_address(trimmed);
            sender = addr;
            let resp = "250 OK\r\n";
            writer.write_all(resp.as_bytes()).await?;
        } else if upper.starts_with("RCPT TO:") {
            let addr = extract_address(trimmed);
            if let Some(addr) = addr {
                recipients.push(addr);
            }
            let resp = "250 OK\r\n";
            writer.write_all(resp.as_bytes()).await?;
        } else if upper == "DATA" {
            if !authenticated {
                let resp = "530 5.7.0 Authentication required\r\n";
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
    if domain == hostname {
        return true;
    }
    queries::get_domain_by_name(db, domain)
        .await
        .ok()
        .flatten()
        .is_some()
}

fn extract_address(line: &str) -> Option<String> {
    let start = line.find('<')?;
    let end = line.find('>')?;
    if start < end {
        Some(line[start + 1..end].to_string())
    } else {
        None
    }
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
