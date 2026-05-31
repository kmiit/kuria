use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::debug;

use crate::config::Config;
use crate::db::queries;

/// Full SMTP session handler
pub async fn handle_smtp_connection(
    stream: TcpStream,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    peer_addr: String,
) -> anyhow::Result<()> {
    let hostname = config.server.hostname.clone();

    // Split the stream into read and write halves
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    // Send greeting
    let greeting = format!("220 {} ESMTP Kuria Mail Server\r\n", hostname);
    write_half.write_all(greeting.as_bytes()).await?;

    let mut sender: Option<String> = None;
    let mut recipients: Vec<String> = Vec::new();
    let mut mail_data = Vec::new();
    let mut in_data = false;

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break; // Connection closed
        }

        let trimmed = line.trim_end_matches(|c| c == '\r' || c == '\n');
        debug!("SMTP << {}: {}", peer_addr, trimmed);

        if in_data {
            if trimmed == "." {
                // End of data
                in_data = false;
                let mail_data_str = String::from_utf8_lossy(&mail_data);

                // Parse and store the email
                let msg_id = format!("<{}.{}@{}>", uuid::Uuid::new_v4(), chrono::Utc::now().timestamp(), hostname);

                // Try to find local recipients and store
                for rcpt in &recipients {
                    // Check if this is a local recipient
                    if let Some(domain) = rcpt.split('@').last() {
                        if is_local_domain(&db, domain, &hostname).await {
                            if let Ok(Some(user)) = queries::get_user_by_email(&db, rcpt).await {
                                let _ = queries::save_email(
                                    &db,
                                    Some(&msg_id),
                                    sender.as_deref().unwrap_or(""),
                                    &serde_json::to_string(&recipients).unwrap_or_default(),
                                    extract_subject(&mail_data_str),
                                    Some(&mail_data_str),
                                    None,
                                    Some(&mail_data),
                                    user.id,
                                    "INBOX",
                                )
                                .await;
                                debug!("Email stored locally for {}", rcpt);
                            }
                        }
                    }
                }

                let resp = "250 OK: Message accepted for delivery\r\n";
                write_half.write_all(resp.as_bytes()).await?;

                // Reset state
                sender = None;
                recipients.clear();
                mail_data.clear();
            } else {
                // Handle dot-stuffing
                if trimmed.starts_with('.') {
                    mail_data.extend_from_slice(trimmed[1..].as_bytes());
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
            write_half.write_all(resp.as_bytes()).await?;
            break;
        } else if upper == "NOOP" {
            let resp = "250 OK\r\n";
            write_half.write_all(resp.as_bytes()).await?;
        } else if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            let resp = format!(
                "250-{}\r\n250-SIZE 52428800\r\n250-8BITMIME\r\n250-STARTTLS\r\n250 HELP\r\n",
                hostname
            );
            write_half.write_all(resp.as_bytes()).await?;
        } else if upper.starts_with("STARTTLS") {
            // TODO: Implement STARTTLS upgrade
            let resp = "220 Ready to start TLS\r\n";
            write_half.write_all(resp.as_bytes()).await?;
        } else if upper.starts_with("MAIL FROM:") {
            let addr = extract_address(trimmed);
            sender = addr;
            let resp = "250 OK\r\n";
            write_half.write_all(resp.as_bytes()).await?;
        } else if upper.starts_with("RCPT TO:") {
            let addr = extract_address(trimmed);
            if let Some(addr) = addr {
                recipients.push(addr);
            }
            let resp = "250 OK\r\n";
            write_half.write_all(resp.as_bytes()).await?;
        } else if upper == "DATA" {
            if recipients.is_empty() {
                let resp = "503 5.5.1 No recipients\r\n";
                write_half.write_all(resp.as_bytes()).await?;
            } else {
                let resp = "354 Start mail input; end with <CRLF>.<CRLF>\r\n";
                write_half.write_all(resp.as_bytes()).await?;
                in_data = true;
                mail_data.clear();
            }
        } else if upper.starts_with("RSET") {
            sender = None;
            recipients.clear();
            mail_data.clear();
            let resp = "250 OK\r\n";
            write_half.write_all(resp.as_bytes()).await?;
        } else {
            let resp = "500 5.5.1 Command not recognized\r\n";
            write_half.write_all(resp.as_bytes()).await?;
        }
    }

    Ok(())
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
        if line.to_lowercase().starts_with("subject:") {
            return Some(line[8..].trim());
        }
        if line.is_empty() {
            break; // End of headers
        }
    }
    None
}
