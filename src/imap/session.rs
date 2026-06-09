use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, warn};

use super::command::ImapCommand;
use crate::config::Config;
use crate::db::queries;

/// Result of an IMAP session
pub enum ImapSessionResult {
    Done,
    StartTls,
}

/// Handle a plain TCP IMAP connection, with STARTTLS support
pub async fn handle_imap_connection(
    stream: tokio::net::TcpStream,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    peer_addr: String,
) -> anyhow::Result<()> {
    let (read_half, write_half) = tokio::io::split(stream);
    let reader = tokio::io::BufReader::new(read_half);

    match handle_imap_session(
        reader,
        write_half,
        config.clone(),
        db.clone(),
        peer_addr.clone(),
        false,
    )
    .await?
    {
        ImapSessionResult::Done => {}
        ImapSessionResult::StartTls => {
            debug!("IMAP STARTTLS from {}", peer_addr);
            // Note: TLS upgrade after split is complex. For production, handle at server level.
            // For now, log and close gracefully.
            tracing::warn!(
                "IMAP STARTTLS requested but TLS upgrade after split not yet implemented for plain connections. Use IMAPS (port 993) instead."
            );
        }
    }
    Ok(())
}

/// Full IMAP session handler - works with any async read/write stream
pub async fn handle_imap_session<R, W>(
    mut reader: R,
    mut writer: W,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    peer_addr: String,
    _is_tls: bool,
) -> anyhow::Result<ImapSessionResult>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // Check if TLS is available
    let starttls_available =
        !_is_tls && config.imap.enable_starttls && config.tls.internal_tls_enabled();

    // Send greeting
    let greeting = if _is_tls {
        "* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] Kuria IMAP Server ready\r\n"
    } else if starttls_available {
        "* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN STARTTLS] Kuria IMAP Server ready\r\n"
    } else {
        "* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] Kuria IMAP Server ready\r\n"
    };
    writer.write_all(greeting.as_bytes()).await?;

    let mut authenticated = false;
    let mut user_id: Option<i64> = None;
    let mut selected_mailbox: Option<String> = None;
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        debug!("IMAP << {}: {}", peer_addr, trimmed);

        // Parse tag and command
        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        if parts.len() < 2 {
            send_str(&mut writer, "*", "BAD Invalid command format").await?;
            continue;
        }

        let tag = parts[0];
        let cmd_str = parts[1];

        match ImapCommand::parse(cmd_str) {
            ImapCommand::Capability => {
                if starttls_available {
                    send_str(&mut writer, "*", "CAPABILITY IMAP4rev1 AUTH=PLAIN STARTTLS").await?;
                } else {
                    send_str(&mut writer, "*", "CAPABILITY IMAP4rev1 AUTH=PLAIN").await?;
                }
                send_str(&mut writer, tag, "OK CAPABILITY completed").await?;
            }
            ImapCommand::Noop => {
                send_str(&mut writer, tag, "OK NOOP completed").await?;
            }
            ImapCommand::Logout => {
                send_str(&mut writer, "*", "BYE Server logging out").await?;
                send_str(&mut writer, tag, "OK LOGOUT completed").await?;
                break;
            }
            ImapCommand::StartTls => {
                if _is_tls {
                    send_str(&mut writer, tag, "NO TLS already active").await?;
                } else if !starttls_available {
                    send_str(&mut writer, tag, "NO STARTTLS not supported").await?;
                } else {
                    send_str(&mut writer, tag, "OK Begin TLS negotiation now").await?;
                    writer.flush().await?;
                    return Ok(ImapSessionResult::StartTls);
                }
            }
            ImapCommand::Login(username, password) => {
                match queries::get_user_by_email(&db, &username).await {
                    Ok(Some(user)) => {
                        if bcrypt::verify(&password, &user.password_hash).unwrap_or(false) {
                            authenticated = true;
                            user_id = Some(user.id);
                            debug!("IMAP LOGIN success for {}", username);
                            send_str(&mut writer, tag, "OK LOGIN completed").await?;
                        } else {
                            warn!("IMAP LOGIN failed for {}: bad password", username);
                            send_str(&mut writer, tag, "NO LOGIN failed: Invalid credentials")
                                .await?;
                        }
                    }
                    _ => {
                        warn!("IMAP LOGIN failed: user not found {}", username);
                        send_str(&mut writer, tag, "NO LOGIN failed: User not found").await?;
                    }
                }
            }
            ImapCommand::List(_reference, pattern) => {
                if !authenticated {
                    send_str(&mut writer, tag, "NO Not authenticated").await?;
                    continue;
                }
                let mailboxes = vec!["INBOX", "Sent", "Drafts", "Trash", "Spam"];
                for mb in &mailboxes {
                    if pattern == "*" || pattern == "%" || mb.contains(&pattern.replace('%', "")) {
                        let resp = format!("LIST (\\HasNoChildren) \"/\" {}", mb);
                        send_str(&mut writer, "*", &resp).await?;
                    }
                }
                send_str(&mut writer, tag, "OK LIST completed").await?;
            }
            ImapCommand::Select(mailbox) => {
                if !authenticated {
                    send_str(&mut writer, tag, "NO Not authenticated").await?;
                    continue;
                }
                let uid = user_id.unwrap();
                match queries::count_emails_by_user(&db, uid, &mailbox).await {
                    Ok(count) => {
                        selected_mailbox = Some(mailbox);
                        send_str(&mut writer, "*", &format!("{} EXISTS", count)).await?;
                        send_str(&mut writer, "*", "0 RECENT").await?;
                        send_str(
                            &mut writer,
                            "*",
                            "FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)",
                        )
                        .await?;
                        send_str(&mut writer, tag, "OK [UIDVALIDITY 1] SELECT completed").await?;
                    }
                    Err(e) => {
                        send_str(&mut writer, tag, &format!("NO SELECT failed: {}", e)).await?;
                    }
                }
            }
            ImapCommand::Fetch(sequence, items) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let uid = user_id.unwrap();
                let mailbox = selected_mailbox.as_ref().unwrap();

                let emails = if sequence == "*" || sequence == "1:*" {
                    queries::get_emails_by_user(&db, uid, mailbox, 100, 0).await?
                } else if let Ok(id) = sequence.parse::<i64>() {
                    if let Some(email) = queries::get_email_by_id(&db, id).await? {
                        vec![email]
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };

                for email in &emails {
                    let mut response = String::new();
                    if items.contains("FLAGS") {
                        let flags = if email.is_read { "\\Seen" } else { "" };
                        response.push_str(&format!(" FLAGS ({})", flags));
                    }
                    if items.contains("ENVELOPE") {
                        // Parse sender for proper ENVELOPE format
                        let (from_name, from_addr) = parse_email_addr(&email.sender);
                        let msg_id = email.message_id.as_deref().unwrap_or("");
                        let in_reply_to = "";
                        let date = email
                            .created_at
                            .map(|c| c.format("%d-%b-%Y %H:%M:%S %z").to_string())
                            .unwrap_or_default();
                        let subject = email.subject.as_deref().unwrap_or("");

                        // Parse recipients for TO field
                        let to_list = parse_recipients_envelope(&email.recipients);

                        response.push_str(&format!(
                            " ENVELOPE (\"{}\" \"{}\" ((\"{}\" NIL \"{}\" \"{}\")) ({}) NIL NIL NIL \"{}\" \"{}\")",
                            date,
                            escape_imap_string(subject),
                            escape_imap_string(&from_name),
                            escape_imap_string(from_addr.split('@').next().unwrap_or("")),
                            escape_imap_string(from_addr.split('@').next_back().unwrap_or("")),
                            to_list,
                            escape_imap_string(msg_id),
                            escape_imap_string(in_reply_to),
                        ));
                    }
                    if items.contains("BODY[]") || items.contains("RFC822") {
                        // Return raw message if available, otherwise body_text
                        let body = if let Some(ref raw) = email.raw_message {
                            raw.clone()
                        } else {
                            email.body_text.as_deref().unwrap_or("").as_bytes().to_vec()
                        };
                        response.push_str(&format!(" BODY[] {{{}}}\r\n", body.len()));
                        // We need to write the response header, then the body
                        let fetch_resp = format!("{} FETCH{}", email.id, response);
                        send_bytes(&mut writer, "*", fetch_resp.as_bytes()).await?;
                        writer.write_all(&body).await?;
                        writer.write_all(b"\r\n").await?;
                        continue;
                    }
                    if items.contains("BODYSTRUCTURE") {
                        let has_html = email.body_html.is_some();
                        if has_html {
                            response.push_str(" BODYSTRUCTURE ((\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 0 0)(\"text\" \"html\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 0 0) \"alternative\")");
                        } else {
                            response.push_str(" BODYSTRUCTURE (\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 0 0)");
                        }
                    }
                    let fetch_resp = format!("{} FETCH{}", email.id, response);
                    send_str(&mut writer, "*", &fetch_resp).await?;
                }
                send_str(&mut writer, tag, "OK FETCH completed").await?;
            }
            ImapCommand::Store(sequence, flags_str) => {
                if !authenticated {
                    send_str(&mut writer, tag, "NO Not authenticated").await?;
                    continue;
                }
                if let Ok(id) = sequence.parse::<i64>() {
                    // Handle different flag operations
                    if flags_str.contains("\\Seen") || flags_str.to_uppercase().contains("FLAGS") {
                        let _ = queries::mark_email_read(&db, id).await;
                    }
                    if flags_str.contains("\\Deleted") {
                        let _ = queries::delete_email(&db, id).await;
                    }
                }
                send_str(&mut writer, tag, "OK STORE completed").await?;
            }
            ImapCommand::Expunge => {
                if !authenticated {
                    send_str(&mut writer, tag, "NO Not authenticated").await?;
                    continue;
                }
                // Actually expunge deleted messages
                if let Some(ref mailbox) = selected_mailbox
                    && let Some(uid) = user_id
                {
                    // Find and delete emails marked as deleted
                    let emails = queries::get_emails_by_user(&db, uid, mailbox, 1000, 0).await?;
                    for email in &emails {
                        if email.is_deleted {
                            let _ = queries::delete_email(&db, email.id).await;
                        }
                    }
                }
                send_str(&mut writer, tag, "OK EXPUNGE completed").await?;
            }
            ImapCommand::Unknown(cmd) => {
                warn!("Unknown IMAP command: {}", cmd);
                send_str(
                    &mut writer,
                    tag,
                    &format!("BAD Command not recognized: {}", cmd),
                )
                .await?;
            }
        }
    }

    Ok(ImapSessionResult::Done)
}

async fn send_str(
    writer: &mut (impl AsyncWrite + Unpin),
    tag: &str,
    response: &str,
) -> anyhow::Result<()> {
    let msg = format!("{} {}\r\n", tag, response);
    debug!("IMAP >> {}", msg.trim());
    writer.write_all(msg.as_bytes()).await?;
    Ok(())
}

async fn send_bytes(
    writer: &mut (impl AsyncWrite + Unpin),
    tag: &str,
    data: &[u8],
) -> anyhow::Result<()> {
    let mut msg = format!("{} ", tag).into_bytes();
    msg.extend_from_slice(data);
    msg.extend_from_slice(b"\r\n");
    writer.write_all(&msg).await?;
    Ok(())
}

fn escape_imap_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse_email_addr(sender: &str) -> (String, String) {
    // Parse "Name <email>" format
    if let Some(start) = sender.find('<')
        && let Some(end) = sender.find('>')
    {
        let name = sender[..start].trim().trim_matches('"').to_string();
        let addr = sender[start + 1..end].to_string();
        return (name, addr);
    }
    // Fallback: treat entire string as address
    (String::new(), sender.to_string())
}

fn parse_recipients_envelope(recipients: &str) -> String {
    let addrs: Vec<String> = if recipients.starts_with('[') {
        serde_json::from_str(recipients).unwrap_or_default()
    } else {
        vec![recipients.to_string()]
    };

    let parts: Vec<String> = addrs
        .iter()
        .map(|addr| {
            let (name, email) = parse_email_addr(addr);
            let local = email.split('@').next().unwrap_or("");
            let domain = email.split('@').next_back().unwrap_or("");
            format!(
                "(\"{}\" NIL \"{}\" \"{}\")",
                escape_imap_string(&name),
                escape_imap_string(local),
                escape_imap_string(domain)
            )
        })
        .collect();

    format!("({})", parts.join(" "))
}
