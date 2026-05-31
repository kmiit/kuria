use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::{debug, warn};

use crate::config::Config;
use crate::db::queries;
use super::command::ImapCommand;

pub struct ImapSession {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    peer_addr: String,
    authenticated: bool,
    user_id: Option<i64>,
    selected_mailbox: Option<String>,
}

impl ImapSession {
    pub fn new(stream: TcpStream, config: Arc<Config>, db: sqlx::SqlitePool, peer_addr: String) -> Self {
        let _ = stream; // consumed in run()
        Self {
            config,
            db,
            peer_addr,
            authenticated: false,
            user_id: None,
            selected_mailbox: None,
        }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        // This is a placeholder - the actual implementation is in handle_imap_connection
        Ok(())
    }
}

/// Full IMAP session handler
pub async fn handle_imap_connection(
    stream: TcpStream,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    peer_addr: String,
) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    // Send greeting
    let greeting = "* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] Kuria IMAP Server ready\r\n";
    write_half.write_all(greeting.as_bytes()).await?;

    let mut authenticated = false;
    let mut user_id: Option<i64> = None;
    let mut selected_mailbox: Option<String> = None;

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let trimmed = line.trim_end_matches(|c| c == '\r' || c == '\n');
        debug!("IMAP << {}: {}", peer_addr, trimmed);

        // Parse tag and command
        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        if parts.len() < 2 {
            send_response(&mut write_half, "*", "BAD Invalid command format").await?;
            continue;
        }

        let tag = parts[0];
        let cmd_str = parts[1];

        match ImapCommand::parse(cmd_str) {
            ImapCommand::Capability => {
                send_response(&mut write_half, "*", "CAPABILITY IMAP4rev1 AUTH=PLAIN IDLE").await?;
                send_response(&mut write_half, tag, "OK CAPABILITY completed").await?;
            }
            ImapCommand::Noop => {
                send_response(&mut write_half, tag, "OK NOOP completed").await?;
            }
            ImapCommand::Logout => {
                send_response(&mut write_half, "*", "BYE Server logging out").await?;
                send_response(&mut write_half, tag, "OK LOGOUT completed").await?;
                break;
            }
            ImapCommand::Login(username, password) => {
                match queries::get_user_by_email(&db, &username).await {
                    Ok(Some(user)) => {
                        if bcrypt::verify(&password, &user.password_hash).unwrap_or(false) {
                            authenticated = true;
                            user_id = Some(user.id);
                            send_response(&mut write_half, tag, "OK LOGIN completed").await?;
                        } else {
                            send_response(&mut write_half, tag, "NO LOGIN failed: Invalid credentials").await?;
                        }
                    }
                    _ => {
                        send_response(&mut write_half, tag, "NO LOGIN failed: User not found").await?;
                    }
                }
            }
            ImapCommand::List(_reference, pattern) => {
                if !authenticated {
                    send_response(&mut write_half, tag, "NO Not authenticated").await?;
                    continue;
                }
                let mailboxes = vec!["INBOX", "Sent", "Drafts", "Trash", "Spam"];
                for mb in &mailboxes {
                    if pattern == "*" || pattern == "%" || mb.contains(&pattern.replace('%', "")) {
                        let resp = format!("LIST (\\HasNoChildren) \"/\" {}", mb);
                        send_response(&mut write_half, "*", &resp).await?;
                    }
                }
                send_response(&mut write_half, tag, "OK LIST completed").await?;
            }
            ImapCommand::Select(mailbox) => {
                if !authenticated {
                    send_response(&mut write_half, tag, "NO Not authenticated").await?;
                    continue;
                }
                let uid = user_id.unwrap();
                match queries::count_emails_by_user(&db, uid, &mailbox).await {
                    Ok(count) => {
                        selected_mailbox = Some(mailbox);
                        send_response(&mut write_half, "*", &format!("{} EXISTS", count)).await?;
                        send_response(&mut write_half, "*", "0 RECENT").await?;
                        send_response(&mut write_half, "*", "FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)").await?;
                        send_response(&mut write_half, tag, "OK [UIDVALIDITY 1] SELECT completed").await?;
                    }
                    Err(e) => {
                        send_response(&mut write_half, tag, &format!("NO SELECT failed: {}", e)).await?;
                    }
                }
            }
            ImapCommand::Fetch(sequence, items) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_response(&mut write_half, tag, "NO Not selected").await?;
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
                        response.push_str(&format!(
                            " ENVELOPE (\"{}\" \"{}\" ((NIL NIL \"{}\" \"{}\")) NIL NIL NIL NIL NIL NIL)",
                            email.created_at.map(|c| c.to_string()).unwrap_or_default(),
                            email.subject.as_deref().unwrap_or(""),
                            email.sender.split('<').next().unwrap_or("").trim(),
                            email.sender.split('<').nth(1).unwrap_or("").trim_end_matches('>'),
                        ));
                    }
                    if items.contains("BODY[]") || items.contains("RFC822") {
                        let body = email.body_text.as_deref().unwrap_or("");
                        response.push_str(&format!(" BODY[] {{{}}}\r\n{}", body.len(), body));
                    }
                    if items.contains("BODYSTRUCTURE") {
                        response.push_str(" BODYSTRUCTURE (\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 0 0)");
                    }
                    let fetch_resp = format!("{} FETCH{}", email.id, response);
                    send_response(&mut write_half, "*", &fetch_resp).await?;
                }
                send_response(&mut write_half, tag, "OK FETCH completed").await?;
            }
            ImapCommand::Store(sequence, _flags) => {
                if !authenticated {
                    send_response(&mut write_half, tag, "NO Not authenticated").await?;
                    continue;
                }
                if let Ok(id) = sequence.parse::<i64>() {
                    let _ = queries::mark_email_read(&db, id).await;
                }
                send_response(&mut write_half, tag, "OK STORE completed").await?;
            }
            ImapCommand::Expunge => {
                if !authenticated {
                    send_response(&mut write_half, tag, "NO Not authenticated").await?;
                    continue;
                }
                send_response(&mut write_half, tag, "OK EXPUNGE completed").await?;
            }
            ImapCommand::Unknown(cmd) => {
                warn!("Unknown IMAP command: {}", cmd);
                send_response(&mut write_half, tag, &format!("BAD Command not recognized: {}", cmd)).await?;
            }
        }
    }

    Ok(())
}

async fn send_response(writer: &mut tokio::net::tcp::OwnedWriteHalf, tag: &str, response: &str) -> anyhow::Result<()> {
    let msg = format!("{} {}\r\n", tag, response);
    debug!("IMAP >> {}", msg.trim());
    writer.write_all(msg.as_bytes()).await?;
    Ok(())
}
