use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, warn};

use super::command::{AppendCommand, ImapCommand};
use crate::config::Config;
use crate::db::models::Email;
use crate::db::queries;

const MAX_AUTH_FAILURES_PER_SESSION: u32 = 5;

/// Result of an IMAP session
pub enum ImapSessionResult {
    Done,
    StartTls,
}

#[derive(Debug, Clone)]
pub struct ImapSessionOptions {
    pub peer_addr: String,
    pub is_tls: bool,
    pub send_greeting: bool,
    pub starttls_supported: bool,
}

impl ImapSessionOptions {
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

/// Handle a plain TCP IMAP connection, with STARTTLS support
pub async fn handle_imap_connection(
    stream: tokio::net::TcpStream,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    peer_addr: String,
) -> anyhow::Result<()> {
    let starttls_acceptor = crate::tls::config::load_internal_tls_config(&config.tls)
        .map(crate::tls::config::create_tls_acceptor)
        .ok();

    let (read_half, write_half) = stream.into_split();
    let reader = tokio::io::BufReader::new(read_half);
    let mut reader = reader;
    let mut writer = write_half;

    match handle_imap_session(
        &mut reader,
        &mut writer,
        config.clone(),
        db.clone(),
        ImapSessionOptions::new(peer_addr.clone(), false, true, starttls_acceptor.is_some()),
    )
    .await?
    {
        ImapSessionResult::Done => {}
        ImapSessionResult::StartTls => {
            debug!("IMAP STARTTLS from {}", peer_addr);
            let Some(acceptor) = starttls_acceptor else {
                anyhow::bail!("IMAP STARTTLS requested but TLS acceptor is unavailable");
            };

            let read_half = reader.into_inner();
            let stream = read_half
                .reunite(writer)
                .map_err(|error| anyhow::anyhow!("failed to restore IMAP stream: {}", error))?;
            let tls_stream = acceptor.accept(stream).await?;
            let (read_half, write_half) = tokio::io::split(tls_stream);
            let mut reader = tokio::io::BufReader::new(read_half);
            let mut writer = write_half;
            handle_imap_session(
                &mut reader,
                &mut writer,
                config,
                db,
                ImapSessionOptions::new(peer_addr.clone(), true, false, false),
            )
            .await?;
        }
    }
    Ok(())
}

/// Full IMAP session handler - works with any async read/write stream
pub async fn handle_imap_session<R, W>(
    reader: &mut R,
    mut writer: &mut W,
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    options: ImapSessionOptions,
) -> anyhow::Result<ImapSessionResult>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let ImapSessionOptions {
        peer_addr,
        is_tls,
        send_greeting,
        starttls_supported,
    } = options;

    // Check if TLS is available
    let starttls_available = imap_starttls_available(&config, is_tls, starttls_supported);

    if send_greeting {
        let greeting = format!(
            "* OK [CAPABILITY {}] Kuria IMAP Server ready\r\n",
            imap_capabilities(starttls_available)
        );
        writer.write_all(greeting.as_bytes()).await?;
    }

    let mut authenticated = false;
    let mut auth_failures = 0u32;
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
                    send_str(
                        &mut writer,
                        "*",
                        &format!("CAPABILITY {}", imap_capabilities(true)),
                    )
                    .await?;
                } else {
                    send_str(
                        &mut writer,
                        "*",
                        &format!("CAPABILITY {}", imap_capabilities(false)),
                    )
                    .await?;
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
                if is_tls {
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
                if starttls_available {
                    send_str(
                        &mut writer,
                        tag,
                        "NO LOGIN disabled until STARTTLS completes",
                    )
                    .await?;
                    continue;
                }
                if imap_auth_locked_out(auth_failures) {
                    send_str(&mut writer, tag, "NO Too many authentication failures").await?;
                    break;
                }
                if let Some(uid) = authenticate_imap_user(&db, &username, &password).await {
                    authenticated = true;
                    auth_failures = 0;
                    user_id = Some(uid);
                    debug!("IMAP LOGIN success for {}", username);
                    send_str(&mut writer, tag, "OK LOGIN completed").await?;
                } else {
                    auth_failures = auth_failures.saturating_add(1);
                    warn!("IMAP LOGIN failed for {}", username);
                    send_str(&mut writer, tag, "NO LOGIN failed: Invalid credentials").await?;
                }
            }
            ImapCommand::AuthenticatePlain(initial_response) => {
                if starttls_available {
                    send_str(
                        &mut writer,
                        tag,
                        "NO AUTHENTICATE disabled until STARTTLS completes",
                    )
                    .await?;
                    continue;
                }
                if imap_auth_locked_out(auth_failures) {
                    send_str(&mut writer, tag, "NO Too many authentication failures").await?;
                    break;
                }

                let response = if let Some(response) = initial_response {
                    response
                } else {
                    writer.write_all(b"+ \r\n").await?;
                    writer.flush().await?;
                    let mut auth_line = String::new();
                    if reader.read_line(&mut auth_line).await? == 0 {
                        break;
                    }
                    auth_line.trim_end_matches(['\r', '\n']).to_string()
                };

                let Some((username, password)) = decode_plain_auth_response(&response) else {
                    auth_failures = auth_failures.saturating_add(1);
                    send_str(&mut writer, tag, "NO AUTHENTICATE failed: Invalid response").await?;
                    continue;
                };

                if let Some(uid) = authenticate_imap_user(&db, &username, &password).await {
                    authenticated = true;
                    auth_failures = 0;
                    user_id = Some(uid);
                    debug!("IMAP AUTHENTICATE PLAIN success for {}", username);
                    send_str(&mut writer, tag, "OK AUTHENTICATE completed").await?;
                } else {
                    auth_failures = auth_failures.saturating_add(1);
                    warn!("IMAP AUTHENTICATE PLAIN failed for {}", username);
                    send_str(
                        &mut writer,
                        tag,
                        "NO AUTHENTICATE failed: Invalid credentials",
                    )
                    .await?;
                }
            }
            ImapCommand::List(_reference, pattern) => {
                if !authenticated {
                    send_str(&mut writer, tag, "NO Not authenticated").await?;
                    continue;
                }
                for mb in crate::imap::mailbox::standard_mailboxes() {
                    if pattern == "*" || pattern == "%" || mb.contains(&pattern.replace('%', "")) {
                        let resp = format!("LIST (\\HasNoChildren) \"/\" {}", quote_mailbox(mb));
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
                let Some(mailbox) = canonical_mailbox_name(&mailbox) else {
                    send_str(&mut writer, tag, "NO Unknown mailbox").await?;
                    continue;
                };
                let uid = match require_user_id(user_id) {
                    Ok(uid) => uid,
                    Err(_) => {
                        send_str(&mut writer, tag, "NO Not authenticated").await?;
                        continue;
                    }
                };
                match queries::get_emails_for_imap(&db, uid, mailbox).await {
                    Ok(messages) => {
                        let count = messages.len();
                        let first_unseen = first_unseen_sequence(&messages);
                        let uid_next = next_uid(&messages);
                        selected_mailbox = Some(mailbox.to_string());
                        send_str(&mut writer, "*", &format!("{} EXISTS", count)).await?;
                        send_str(&mut writer, "*", "0 RECENT").await?;
                        send_str(
                            &mut writer,
                            "*",
                            &format!("OK [UNSEEN {}] First unseen", first_unseen),
                        )
                        .await?;
                        send_str(
                            &mut writer,
                            "*",
                            &format!("OK [UIDNEXT {}] Next unique identifier", uid_next),
                        )
                        .await?;
                        send_str(
                            &mut writer,
                            "*",
                            "FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)",
                        )
                        .await?;
                        send_str(
                            &mut writer,
                            tag,
                            "OK [READ-WRITE] [UIDVALIDITY 1] SELECT completed",
                        )
                        .await?;
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
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, false).await?;
                write_fetch_responses(&mut writer, &emails, &items, false).await?;
                send_str(&mut writer, tag, "OK FETCH completed").await?;
            }
            ImapCommand::Store(sequence, flags_str) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, false).await?;
                let updated = apply_store_flags(&db, &emails, &flags_str).await?;
                write_fetch_responses(&mut writer, &updated, "FLAGS", false).await?;
                send_str(&mut writer, tag, "OK STORE completed").await?;
            }
            ImapCommand::Search(criteria) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = search_messages(&db, uid, mailbox, &criteria).await?;
                let ids = emails
                    .iter()
                    .map(|message| message.sequence_number.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                send_str(&mut writer, "*", &format!("SEARCH {}", ids)).await?;
                send_str(&mut writer, tag, "OK SEARCH completed").await?;
            }
            ImapCommand::UidFetch(sequence, items) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, true).await?;
                write_fetch_responses(&mut writer, &emails, &items, true).await?;
                send_str(&mut writer, tag, "OK UID FETCH completed").await?;
            }
            ImapCommand::UidStore(sequence, flags_str) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, true).await?;
                let updated = apply_store_flags(&db, &emails, &flags_str).await?;
                write_fetch_responses(&mut writer, &updated, "FLAGS", true).await?;
                send_str(&mut writer, tag, "OK UID STORE completed").await?;
            }
            ImapCommand::UidSearch(criteria) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = search_messages(&db, uid, mailbox, &criteria).await?;
                let ids = emails
                    .iter()
                    .map(|message| message.email.id.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                send_str(&mut writer, "*", &format!("SEARCH {}", ids)).await?;
                send_str(&mut writer, tag, "OK UID SEARCH completed").await?;
            }
            ImapCommand::Copy(sequence, destination) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let Some(destination) = canonical_mailbox_name(&destination) else {
                    send_str(&mut writer, tag, "NO Unknown destination mailbox").await?;
                    continue;
                };
                if !is_valid_message_destination_mailbox(destination) {
                    send_str(&mut writer, tag, "NO Drafts cannot be used as destination").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, false).await?;
                for message in &emails {
                    let _ = queries::copy_email_to_mailbox(&db, message.email.id, uid, destination)
                        .await?;
                }
                send_str(&mut writer, tag, "OK COPY completed").await?;
            }
            ImapCommand::UidCopy(sequence, destination) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let Some(destination) = canonical_mailbox_name(&destination) else {
                    send_str(&mut writer, tag, "NO Unknown destination mailbox").await?;
                    continue;
                };
                if !is_valid_message_destination_mailbox(destination) {
                    send_str(&mut writer, tag, "NO Drafts cannot be used as destination").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, true).await?;
                for message in &emails {
                    let _ = queries::copy_email_to_mailbox(&db, message.email.id, uid, destination)
                        .await?;
                }
                send_str(&mut writer, tag, "OK UID COPY completed").await?;
            }
            ImapCommand::Move(sequence, destination) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let Some(destination) = canonical_mailbox_name(&destination) else {
                    send_str(&mut writer, tag, "NO Unknown destination mailbox").await?;
                    continue;
                };
                if !is_valid_message_destination_mailbox(destination) {
                    send_str(&mut writer, tag, "NO Drafts cannot be used as destination").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                if is_drafts_mailbox(mailbox) {
                    send_str(&mut writer, tag, "NO Draft messages cannot be moved").await?;
                    continue;
                }
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, false).await?;
                move_messages_to_mailbox(&db, &emails, destination).await?;
                send_str(&mut writer, tag, "OK MOVE completed").await?;
            }
            ImapCommand::UidMove(sequence, destination) => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let Some(destination) = canonical_mailbox_name(&destination) else {
                    send_str(&mut writer, tag, "NO Unknown destination mailbox").await?;
                    continue;
                };
                if !is_valid_message_destination_mailbox(destination) {
                    send_str(&mut writer, tag, "NO Drafts cannot be used as destination").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                if is_drafts_mailbox(mailbox) {
                    send_str(&mut writer, tag, "NO Draft messages cannot be moved").await?;
                    continue;
                }
                let emails = messages_for_sequence(&db, uid, mailbox, &sequence, true).await?;
                move_messages_to_mailbox(&db, &emails, destination).await?;
                send_str(&mut writer, tag, "OK UID MOVE completed").await?;
            }
            ImapCommand::Append(mut command) => {
                if !authenticated {
                    send_str(&mut writer, tag, "NO Not authenticated").await?;
                    continue;
                }
                let Some(mailbox) = canonical_mailbox_name(&command.mailbox) else {
                    send_str(&mut writer, tag, "NO Unknown destination mailbox").await?;
                    continue;
                };
                command.mailbox = mailbox.to_string();
                let uid = require_user_id(user_id)?;
                match append_message(&db, reader, &mut writer, uid, &command).await {
                    Ok(()) => send_str(&mut writer, tag, "OK APPEND completed").await?,
                    Err(error) => {
                        warn!("IMAP APPEND failed: {}", error);
                        send_str(&mut writer, tag, "NO APPEND failed").await?;
                    }
                }
            }
            ImapCommand::Status(mailbox, items) => {
                if !authenticated {
                    send_str(&mut writer, tag, "NO Not authenticated").await?;
                    continue;
                }
                let Some(mailbox) = canonical_mailbox_name(&mailbox) else {
                    send_str(&mut writer, tag, "NO Unknown mailbox").await?;
                    continue;
                };
                let uid = require_user_id(user_id)?;
                let messages = queries::get_emails_for_imap(&db, uid, mailbox).await?;
                let visible_messages = messages.len();
                let unseen = unseen_count(&messages);
                let uid_next = next_uid(&messages);
                let status =
                    build_status_response(mailbox, &items, visible_messages, unseen, uid_next);
                send_str(&mut writer, "*", &status).await?;
                send_str(&mut writer, tag, "OK STATUS completed").await?;
            }
            ImapCommand::Expunge => {
                if !authenticated || selected_mailbox.is_none() {
                    send_str(&mut writer, tag, "NO Not selected").await?;
                    continue;
                }
                let uid = require_user_id(user_id)?;
                let mailbox = require_selected_mailbox(&selected_mailbox)?;
                let messages = mailbox_messages(&db, uid, mailbox).await?;
                for sequence in expunge_sequence_numbers(&messages) {
                    send_str(&mut writer, "*", &format!("{} EXPUNGE", sequence)).await?;
                }
                let _ = queries::expunge_deleted_emails(&db, uid, mailbox).await?;
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

fn require_user_id(user_id: Option<i64>) -> anyhow::Result<i64> {
    user_id.ok_or_else(|| anyhow::anyhow!("IMAP user id missing after authentication check"))
}

fn require_selected_mailbox(selected_mailbox: &Option<String>) -> anyhow::Result<&str> {
    selected_mailbox
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("IMAP mailbox missing after selection check"))
}

fn imap_starttls_available(config: &Config, is_tls: bool, starttls_supported: bool) -> bool {
    !is_tls && config.imap.enable_starttls && starttls_supported
}

fn imap_capabilities(starttls_available: bool) -> &'static str {
    if starttls_available {
        "IMAP4rev1 STARTTLS LOGINDISABLED MOVE"
    } else {
        "IMAP4rev1 AUTH=PLAIN MOVE"
    }
}

async fn authenticate_imap_user(
    db: &sqlx::SqlitePool,
    username: &str,
    password: &str,
) -> Option<i64> {
    let username = normalize_auth_username(username);
    if username.is_empty() {
        return None;
    }

    let user = queries::get_user_by_email(db, &username)
        .await
        .ok()
        .flatten()?;
    if bcrypt::verify(password, &user.password_hash).unwrap_or(false) {
        Some(user.id)
    } else {
        None
    }
}

fn normalize_auth_username(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn decode_plain_auth_response(response: &str) -> Option<(String, String)> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let decoded = STANDARD.decode(response.trim()).ok()?;
    let parts: Vec<&[u8]> = decoded.split(|&byte| byte == 0).collect();
    if parts.len() < 3 || parts[1].is_empty() {
        return None;
    }

    let username = String::from_utf8(parts[1].to_vec()).ok()?;
    let password = String::from_utf8(parts[2].to_vec()).ok()?;
    Some((username, password))
}

fn imap_auth_locked_out(failures: u32) -> bool {
    failures >= MAX_AUTH_FAILURES_PER_SESSION
}

#[derive(Debug, Clone)]
struct ImapMessage {
    sequence_number: usize,
    email: Email,
}

async fn write_fetch_responses(
    writer: &mut (impl AsyncWrite + Unpin),
    messages: &[ImapMessage],
    items: &str,
    include_uid: bool,
) -> anyhow::Result<()> {
    let upper_items = items.to_uppercase();

    for message in messages {
        let mut response = String::new();
        if include_uid || upper_items.contains("UID") {
            response.push_str(&format!(" UID {}", message.email.id));
        }
        if upper_items.contains("FLAGS")
            || upper_items.contains("ALL")
            || upper_items.contains("FULL")
        {
            response.push_str(&format!(" FLAGS {}", flags_for_email(&message.email)));
        }
        if upper_items.contains("ENVELOPE")
            || upper_items.contains("ALL")
            || upper_items.contains("FULL")
        {
            response.push_str(&format!(" ENVELOPE {}", envelope_for_email(&message.email)));
        }
        if upper_items.contains("INTERNALDATE")
            || upper_items.contains("ALL")
            || upper_items.contains("FULL")
        {
            response.push_str(&format!(
                " INTERNALDATE \"{}\"",
                internal_date(&message.email)
            ));
        }
        if upper_items.contains("RFC822.SIZE")
            || upper_items.contains("ALL")
            || upper_items.contains("FULL")
        {
            response.push_str(&format!(" RFC822.SIZE {}", raw_body(&message.email).len()));
        }

        if upper_items.contains("BODYSTRUCTURE") {
            response.push_str(&bodystructure_for_email(&message.email));
        }
        let literals = fetch_literals(&message.email, &upper_items);
        if !literals.is_empty() {
            write_literal_fetch_response(
                writer,
                message.sequence_number,
                response.trim(),
                &literals,
            )
            .await?;
            continue;
        }

        if response.is_empty() {
            response.push_str(&format!(" FLAGS {}", flags_for_email(&message.email)));
        }
        let fetch_resp = format!("{} FETCH ({})", message.sequence_number, response.trim());
        send_str(writer, "*", &fetch_resp).await?;
    }

    Ok(())
}

async fn apply_store_flags(
    db: &sqlx::SqlitePool,
    messages: &[ImapMessage],
    flags_str: &str,
) -> anyhow::Result<Vec<ImapMessage>> {
    let upper = flags_str.to_uppercase();
    let mut updated = Vec::with_capacity(messages.len());
    for message in messages {
        let mut email = message.email.clone();
        if upper.contains("\\SEEN") {
            let mark_seen = !upper.starts_with("-FLAGS");
            queries::set_email_read(db, message.email.id, mark_seen).await?;
            email.is_read = mark_seen;
        }
        if upper.contains("\\DELETED") {
            let mark_deleted = !upper.starts_with("-FLAGS");
            queries::set_email_deleted(db, message.email.id, mark_deleted).await?;
            email.is_deleted = mark_deleted;
        }
        updated.push(ImapMessage {
            sequence_number: message.sequence_number,
            email,
        });
    }

    Ok(updated)
}

async fn move_messages_to_mailbox(
    db: &sqlx::SqlitePool,
    messages: &[ImapMessage],
    destination: &str,
) -> anyhow::Result<()> {
    for message in messages {
        queries::move_email(db, message.email.id, destination).await?;
    }

    Ok(())
}

fn expunge_sequence_numbers(messages: &[ImapMessage]) -> Vec<usize> {
    let mut expunged_before = 0usize;
    messages
        .iter()
        .filter_map(|message| {
            if message.email.is_deleted {
                let sequence = message.sequence_number.saturating_sub(expunged_before);
                expunged_before += 1;
                Some(sequence)
            } else {
                None
            }
        })
        .collect()
}

async fn search_messages(
    db: &sqlx::SqlitePool,
    user_id: i64,
    mailbox: &str,
    criteria: &str,
) -> anyhow::Result<Vec<ImapMessage>> {
    let messages = mailbox_messages(db, user_id, mailbox).await?;
    let total = messages.len().max(1);
    let max_uid = messages
        .iter()
        .map(|message| message.email.id as usize)
        .max()
        .unwrap_or(0);
    let criteria = parse_search_criteria(criteria);
    let filtered = messages
        .into_iter()
        .filter(|message| {
            criteria
                .iter()
                .all(|criterion| criterion.matches(message, total, max_uid))
        })
        .collect();
    Ok(filtered)
}

async fn append_message<R, W>(
    db: &sqlx::SqlitePool,
    reader: &mut R,
    writer: &mut W,
    user_id: i64,
    command: &AppendCommand,
) -> anyhow::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    writer.write_all(b"+ Ready for literal data\r\n").await?;
    writer.flush().await?;

    let mut raw = vec![0u8; command.literal_size];
    reader.read_exact(&mut raw).await?;
    consume_append_line_ending(reader).await?;

    let parsed = crate::mail::parser::parse_email(&raw).ok();
    let sender = parsed
        .as_ref()
        .map(|parsed| parsed.sender.as_str())
        .filter(|sender| !sender.is_empty())
        .unwrap_or("");
    let recipients_json = parsed
        .as_ref()
        .map(|parsed| serde_json::to_string(&parsed.recipients).unwrap_or_default())
        .unwrap_or_else(|| "[]".to_string());
    let is_seen = command
        .flags
        .as_deref()
        .map(|flags| flags.contains("\\Seen"))
        .unwrap_or(false);

    let email = queries::save_email(
        db,
        queries::NewEmail {
            message_id: parsed
                .as_ref()
                .and_then(|parsed| parsed.message_id.as_deref()),
            sender,
            recipients: &recipients_json,
            subject: parsed.as_ref().and_then(|parsed| parsed.subject.as_deref()),
            body_text: parsed
                .as_ref()
                .and_then(|parsed| parsed.body_text.as_deref()),
            body_html: parsed
                .as_ref()
                .and_then(|parsed| parsed.body_html.as_deref()),
            raw_message: Some(raw.as_slice()),
            user_id,
            mailbox: &command.mailbox,
            is_read: is_seen,
        },
    )
    .await?;

    if let Some(parsed) = parsed {
        for attachment in parsed.attachments {
            if !attachment.data.is_empty() {
                let _ = queries::save_attachment(
                    db,
                    email.id,
                    attachment.filename.as_deref(),
                    attachment.content_type.as_deref(),
                    &attachment.data,
                )
                .await;
            }
        }
    }

    Ok(())
}

async fn consume_append_line_ending<R>(reader: &mut R) -> anyhow::Result<()>
where
    R: AsyncBufRead + Unpin,
{
    let mut ending = [0u8; 2];
    match reader.read_exact(&mut ending).await {
        Ok(_) if ending == *b"\r\n" => Ok(()),
        Ok(_) if ending[0] == b'\n' => Ok(()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn messages_for_sequence(
    db: &sqlx::SqlitePool,
    user_id: i64,
    mailbox: &str,
    sequence: &str,
    uid_mode: bool,
) -> anyhow::Result<Vec<ImapMessage>> {
    let messages = mailbox_messages(db, user_id, mailbox).await?;
    Ok(filter_sequence(messages, sequence, uid_mode))
}

async fn mailbox_messages(
    db: &sqlx::SqlitePool,
    user_id: i64,
    mailbox: &str,
) -> anyhow::Result<Vec<ImapMessage>> {
    let emails = queries::get_emails_for_imap(db, user_id, mailbox).await?;
    Ok(emails
        .into_iter()
        .enumerate()
        .map(|(index, email)| ImapMessage {
            sequence_number: index + 1,
            email,
        })
        .collect())
}

fn filter_sequence(messages: Vec<ImapMessage>, sequence: &str, uid_mode: bool) -> Vec<ImapMessage> {
    let total = messages.len().max(1);
    let max_uid = messages
        .iter()
        .map(|message| message.email.id as usize)
        .max()
        .unwrap_or(0);
    messages
        .into_iter()
        .filter(|message| sequence_matches(sequence, message, uid_mode, total, max_uid))
        .collect()
}

fn sequence_matches(
    sequence_set: &str,
    message: &ImapMessage,
    uid_mode: bool,
    total: usize,
    max_uid: usize,
) -> bool {
    sequence_set
        .split(',')
        .any(|part| sequence_part_matches(part.trim(), message, uid_mode, total, max_uid))
}

fn sequence_part_matches(
    part: &str,
    message: &ImapMessage,
    uid_mode: bool,
    total: usize,
    max_uid: usize,
) -> bool {
    if part.is_empty() {
        return false;
    }
    let value = if uid_mode {
        message.email.id as usize
    } else {
        message.sequence_number
    };

    if part == "*" {
        return if uid_mode {
            value == max_uid
        } else {
            message.sequence_number == total
        };
    }

    if let Some((start, end)) = part.split_once(':') {
        let wildcard = if uid_mode { max_uid } else { total };
        let start = sequence_bound(start, wildcard).unwrap_or(1);
        let end = sequence_bound(end, wildcard).unwrap_or(start);
        let (min, max) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        return value >= min && value <= max;
    }

    part.parse::<usize>() == Ok(value)
}

fn sequence_bound(value: &str, total: usize) -> Option<usize> {
    if value == "*" {
        Some(total)
    } else {
        value.parse::<usize>().ok()
    }
}

fn envelope_for_email(email: &Email) -> String {
    let (from_name, from_addr) = parse_email_addr(&email.sender);
    let msg_id = email.message_id.as_deref().unwrap_or("");
    let in_reply_to = "";
    let date = internal_date(email);
    let subject = email.subject.as_deref().unwrap_or("");
    let to_list = parse_recipients_envelope(&email.recipients);

    format!(
        "(\"{}\" \"{}\" ((\"{}\" NIL \"{}\" \"{}\")) ({}) NIL NIL NIL \"{}\" \"{}\")",
        date,
        escape_imap_string(subject),
        escape_imap_string(&from_name),
        escape_imap_string(from_addr.split('@').next().unwrap_or("")),
        escape_imap_string(from_addr.split('@').next_back().unwrap_or("")),
        to_list,
        escape_imap_string(msg_id),
        escape_imap_string(in_reply_to),
    )
}

fn bodystructure_for_email(email: &Email) -> String {
    if email.body_html.is_some() {
        " BODYSTRUCTURE ((\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 0 0)(\"text\" \"html\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 0 0) \"alternative\")".to_string()
    } else {
        " BODYSTRUCTURE (\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 0 0)"
            .to_string()
    }
}

struct FetchLiteral {
    item: String,
    data: Vec<u8>,
}

async fn write_literal_fetch_response(
    writer: &mut (impl AsyncWrite + Unpin),
    sequence_number: usize,
    metadata: &str,
    literals: &[FetchLiteral],
) -> anyhow::Result<()> {
    let mut response = format!("* {} FETCH (", sequence_number).into_bytes();
    if !metadata.is_empty() {
        response.extend_from_slice(metadata.as_bytes());
        response.push(b' ');
    }

    for (index, literal) in literals.iter().enumerate() {
        if index > 0 {
            response.push(b' ');
        }
        response.extend_from_slice(literal.item.as_bytes());
        response.extend_from_slice(format!(" {{{}}}\r\n", literal.data.len()).as_bytes());
        response.extend_from_slice(&literal.data);
        response.extend_from_slice(b"\r\n");
    }

    response.extend_from_slice(b")\r\n");
    writer.write_all(&response).await?;
    Ok(())
}

fn fetch_literals(email: &Email, upper_items: &str) -> Vec<FetchLiteral> {
    let raw = raw_body(email);
    let mut literals = Vec::new();

    if let Some(fields) = header_fields_request(upper_items) {
        literals.push(FetchLiteral {
            item: format!("BODY[HEADER.FIELDS ({})]", fields.join(" ")),
            data: raw_message_header_fields(&raw, &fields),
        });
    }

    if upper_items.contains("BODY.PEEK[HEADER]")
        || upper_items.contains("BODY[HEADER]")
        || upper_items.contains("RFC822.HEADER")
    {
        literals.push(FetchLiteral {
            item: if upper_items.contains("RFC822.HEADER") {
                "RFC822.HEADER".to_string()
            } else {
                "BODY[HEADER]".to_string()
            },
            data: raw_message_header(&raw),
        });
    }

    if upper_items.contains("BODY.PEEK[TEXT]")
        || upper_items.contains("BODY[TEXT]")
        || upper_items.contains("RFC822.TEXT")
    {
        literals.push(FetchLiteral {
            item: if upper_items.contains("RFC822.TEXT") {
                "RFC822.TEXT".to_string()
            } else {
                "BODY[TEXT]".to_string()
            },
            data: raw_message_text(&raw),
        });
    }

    if literals.is_empty()
        && (upper_items.contains("BODY.PEEK[]")
            || upper_items.contains("BODY[]")
            || fetch_requests_rfc822_literal(upper_items))
    {
        literals.push(FetchLiteral {
            item: if fetch_requests_rfc822_literal(upper_items) {
                "RFC822".to_string()
            } else {
                "BODY[]".to_string()
            },
            data: raw,
        });
    }

    literals
}

fn fetch_requests_rfc822_literal(upper_items: &str) -> bool {
    upper_items
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ')'))
        .any(|token| token == "RFC822")
}

fn header_fields_request(upper_items: &str) -> Option<Vec<String>> {
    let marker = "HEADER.FIELDS";
    let marker_start = upper_items.find(marker)?;
    let fields_start = upper_items[marker_start..].find('(')? + marker_start;
    let fields_end = upper_items[fields_start + 1..].find(')')? + fields_start + 1;
    let fields = upper_items[fields_start + 1..fields_end]
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();

    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

fn raw_message_header(raw: &[u8]) -> Vec<u8> {
    match find_header_body_separator(raw) {
        Some(separator) => raw[..separator.header_end].to_vec(),
        None => raw.to_vec(),
    }
}

fn raw_message_header_fields(raw: &[u8], requested_fields: &[String]) -> Vec<u8> {
    let header = raw_message_header(raw);
    let header = String::from_utf8_lossy(&header);
    let requested = requested_fields
        .iter()
        .map(|field| field.trim().to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();

    let mut output = String::new();
    let mut current = String::new();
    for line in header.split_inclusive('\n') {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if !current.is_empty() {
                current.push_str("\r\n");
                current.push_str(line);
            }
            continue;
        }

        append_requested_header(&mut output, &current, &requested);
        current.clear();
        current.push_str(line);
    }
    append_requested_header(&mut output, &current, &requested);
    output.push_str("\r\n");
    output.into_bytes()
}

fn append_requested_header(
    output: &mut String,
    header: &str,
    requested: &std::collections::HashSet<String>,
) {
    let Some((name, _value)) = header.split_once(':') else {
        return;
    };
    if requested.contains(&name.trim().to_ascii_lowercase()) {
        output.push_str(header);
        output.push_str("\r\n");
    }
}

fn raw_message_text(raw: &[u8]) -> Vec<u8> {
    match find_header_body_separator(raw) {
        Some(separator) => raw[separator.body_start..].to_vec(),
        None => Vec::new(),
    }
}

struct HeaderBodySeparator {
    header_end: usize,
    body_start: usize,
}

fn find_header_body_separator(raw: &[u8]) -> Option<HeaderBodySeparator> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| HeaderBodySeparator {
            header_end: index + 2,
            body_start: index + 4,
        })
        .or_else(|| {
            raw.windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| HeaderBodySeparator {
                    header_end: index + 1,
                    body_start: index + 2,
                })
        })
}

fn flags_for_email(email: &Email) -> String {
    let mut flags = Vec::new();
    if email.is_read {
        flags.push("\\Seen");
    }
    if email.is_deleted {
        flags.push("\\Deleted");
    }
    format!("({})", flags.join(" "))
}

fn raw_body(email: &Email) -> Vec<u8> {
    email
        .raw_message
        .clone()
        .unwrap_or_else(|| email.body_text.as_deref().unwrap_or("").as_bytes().to_vec())
}

fn internal_date(email: &Email) -> String {
    email
        .created_at
        .map(|created| created.format("%d-%b-%Y %H:%M:%S +0000").to_string())
        .unwrap_or_else(|| {
            chrono::Utc::now()
                .format("%d-%b-%Y %H:%M:%S +0000")
                .to_string()
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchCriterion {
    All,
    Seen,
    Unseen,
    Deleted,
    Undeleted,
    From(String),
    To(String),
    Subject(String),
    Body(String),
    Text(String),
    Header(String, String),
    Larger(usize),
    Smaller(usize),
    SequenceSet(String),
    UidSet(String),
    Not(Box<SearchCriterion>),
    Or(Box<SearchCriterion>, Box<SearchCriterion>),
    And(Vec<SearchCriterion>),
    Unsupported,
}

impl SearchCriterion {
    fn matches(&self, message: &ImapMessage, total: usize, max_uid: usize) -> bool {
        match self {
            SearchCriterion::All | SearchCriterion::Unsupported => true,
            SearchCriterion::Seen => message.email.is_read,
            SearchCriterion::Unseen => !message.email.is_read,
            SearchCriterion::Deleted => message.email.is_deleted,
            SearchCriterion::Undeleted => !message.email.is_deleted,
            SearchCriterion::From(query) => contains_ci(&message.email.sender, query),
            SearchCriterion::To(query) => contains_ci(&message.email.recipients, query),
            SearchCriterion::Subject(query) => {
                contains_ci(message.email.subject.as_deref().unwrap_or(""), query)
            }
            SearchCriterion::Body(query) => email_body_matches(&message.email, query),
            SearchCriterion::Text(query) => email_text_matches(&message.email, query),
            SearchCriterion::Header(name, query) => {
                email_header_matches(&message.email, name, query)
            }
            SearchCriterion::Larger(size) => raw_body(&message.email).len() > *size,
            SearchCriterion::Smaller(size) => raw_body(&message.email).len() < *size,
            SearchCriterion::SequenceSet(sequence_set) => {
                sequence_matches(sequence_set, message, false, total, max_uid)
            }
            SearchCriterion::UidSet(sequence_set) => {
                sequence_matches(sequence_set, message, true, total, max_uid)
            }
            SearchCriterion::Not(criterion) => !criterion.matches(message, total, max_uid),
            SearchCriterion::Or(left, right) => {
                left.matches(message, total, max_uid) || right.matches(message, total, max_uid)
            }
            SearchCriterion::And(criteria) => criteria
                .iter()
                .all(|criterion| criterion.matches(message, total, max_uid)),
        }
    }
}

fn parse_search_criteria(criteria: &str) -> Vec<SearchCriterion> {
    let tokens = tokenize_search_criteria(criteria);
    if tokens.is_empty() {
        return vec![SearchCriterion::All];
    }

    let mut cursor = SearchCursor { tokens, index: 0 };
    let mut parsed = Vec::new();
    while cursor.has_more() {
        if cursor.peek().is_some_and(|token| token == ")") {
            cursor.index += 1;
            continue;
        }
        if let Some(criterion) = cursor.parse_one() {
            parsed.push(criterion);
        }
    }

    if parsed.is_empty() {
        vec![SearchCriterion::All]
    } else {
        parsed
    }
}

struct SearchCursor {
    tokens: Vec<String>,
    index: usize,
}

impl SearchCursor {
    fn has_more(&self) -> bool {
        self.index < self.tokens.len()
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.index).map(String::as_str)
    }

    fn next(&mut self) -> Option<String> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn parse_one(&mut self) -> Option<SearchCriterion> {
        let token = self.next()?;
        let upper = token.to_ascii_uppercase();
        match upper.as_str() {
            "(" => Some(self.parse_group()),
            ")" => None,
            "ALL" => Some(SearchCriterion::All),
            "SEEN" => Some(SearchCriterion::Seen),
            "UNSEEN" => Some(SearchCriterion::Unseen),
            "DELETED" => Some(SearchCriterion::Deleted),
            "UNDELETED" => Some(SearchCriterion::Undeleted),
            "FROM" => Some(SearchCriterion::From(self.next_search_value())),
            "TO" | "CC" | "BCC" => Some(SearchCriterion::To(self.next_search_value())),
            "SUBJECT" => Some(SearchCriterion::Subject(self.next_search_value())),
            "BODY" => Some(SearchCriterion::Body(self.next_search_value())),
            "TEXT" => Some(SearchCriterion::Text(self.next_search_value())),
            "HEADER" => {
                let name = self.next_search_value();
                let value = self.next_search_value();
                Some(SearchCriterion::Header(name, value))
            }
            "LARGER" => Some(SearchCriterion::Larger(
                self.next_search_value()
                    .parse::<usize>()
                    .unwrap_or(usize::MAX),
            )),
            "SMALLER" => Some(SearchCriterion::Smaller(
                self.next_search_value().parse::<usize>().unwrap_or(0),
            )),
            "UID" => Some(SearchCriterion::UidSet(self.next_search_value())),
            "NOT" => self
                .parse_one()
                .map(|criterion| SearchCriterion::Not(Box::new(criterion)))
                .or(Some(SearchCriterion::Unsupported)),
            "OR" => {
                let left = self.parse_one().unwrap_or(SearchCriterion::Unsupported);
                let right = self.parse_one().unwrap_or(SearchCriterion::Unsupported);
                Some(SearchCriterion::Or(Box::new(left), Box::new(right)))
            }
            "CHARSET" | "BEFORE" | "ON" | "SINCE" | "SENTBEFORE" | "SENTON" | "SENTSINCE" => {
                let _ = self.next_search_value();
                Some(SearchCriterion::Unsupported)
            }
            "ANSWERED" | "UNANSWERED" | "FLAGGED" | "UNFLAGGED" | "DRAFT" | "UNDRAFT" | "NEW"
            | "OLD" | "RECENT" => Some(SearchCriterion::Unsupported),
            _ if is_sequence_set_token(&token) => Some(SearchCriterion::SequenceSet(token)),
            _ => Some(SearchCriterion::Unsupported),
        }
    }

    fn parse_group(&mut self) -> SearchCriterion {
        let mut criteria = Vec::new();
        while self.has_more() {
            if self.peek().is_some_and(|token| token == ")") {
                self.index += 1;
                break;
            }
            if let Some(criterion) = self.parse_one() {
                criteria.push(criterion);
            }
        }
        SearchCriterion::And(criteria)
    }

    fn next_search_value(&mut self) -> String {
        self.next()
            .filter(|value| value != "(" && value != ")")
            .unwrap_or_default()
    }
}

fn tokenize_search_criteria(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_quote => escaped = true,
            '"' => {
                if in_quote {
                    tokens.push(current.clone());
                    current.clear();
                    in_quote = false;
                } else {
                    push_search_token(&mut tokens, &mut current);
                    in_quote = true;
                }
            }
            '(' | ')' if !in_quote => {
                push_search_token(&mut tokens, &mut current);
                tokens.push(ch.to_string());
            }
            ch if ch.is_whitespace() && !in_quote => push_search_token(&mut tokens, &mut current),
            _ => current.push(ch),
        }
    }

    push_search_token(&mut tokens, &mut current);
    tokens
}

fn push_search_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

fn is_sequence_set_token(token: &str) -> bool {
    token
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b':' || byte == b',' || byte == b'*')
        && token
            .bytes()
            .any(|byte| byte.is_ascii_digit() || byte == b'*')
}

fn email_body_matches(email: &Email, query: &str) -> bool {
    contains_ci(email.body_text.as_deref().unwrap_or(""), query)
        || contains_ci(email.body_html.as_deref().unwrap_or(""), query)
        || email
            .raw_message
            .as_deref()
            .is_some_and(|raw| contains_ci(&String::from_utf8_lossy(&raw_message_text(raw)), query))
}

fn email_text_matches(email: &Email, query: &str) -> bool {
    contains_ci(&email.sender, query)
        || contains_ci(&email.recipients, query)
        || contains_ci(email.subject.as_deref().unwrap_or(""), query)
        || email_body_matches(email, query)
        || email
            .raw_message
            .as_deref()
            .is_some_and(|raw| contains_ci(&String::from_utf8_lossy(raw), query))
}

fn email_header_matches(email: &Email, name: &str, query: &str) -> bool {
    if let Some(raw) = email.raw_message.as_deref()
        && raw_header_matches(raw, name, query)
    {
        return true;
    }

    match name.to_ascii_lowercase().as_str() {
        "from" => contains_ci(&email.sender, query),
        "to" | "cc" | "bcc" => contains_ci(&email.recipients, query),
        "subject" => contains_ci(email.subject.as_deref().unwrap_or(""), query),
        "message-id" => contains_ci(email.message_id.as_deref().unwrap_or(""), query),
        _ => false,
    }
}

fn raw_header_matches(raw: &[u8], name: &str, query: &str) -> bool {
    let header = raw_message_header(raw);
    let header = String::from_utf8_lossy(&header);
    let wanted = name.trim().to_ascii_lowercase();
    let mut current = String::new();

    for line in header.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if !current.is_empty() {
                current.push(' ');
                current.push_str(line.trim());
            }
            continue;
        }

        if header_value_matches(&current, &wanted, query) {
            return true;
        }
        current.clear();
        current.push_str(line);
    }

    header_value_matches(&current, &wanted, query)
}

fn header_value_matches(header: &str, name: &str, query: &str) -> bool {
    let Some((header_name, header_value)) = header.split_once(':') else {
        return false;
    };
    header_name.trim().eq_ignore_ascii_case(name) && contains_ci(header_value, query)
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn build_status_response(
    mailbox: &str,
    items: &str,
    messages: usize,
    unseen: usize,
    uid_next: i64,
) -> String {
    let upper = items.to_uppercase();
    let mut values = Vec::new();
    if upper.contains("MESSAGES") {
        values.push(format!("MESSAGES {}", messages));
    }
    if upper.contains("UNSEEN") {
        values.push(format!("UNSEEN {}", unseen));
    }
    if upper.contains("UIDNEXT") {
        values.push(format!("UIDNEXT {}", uid_next));
    }
    if upper.contains("UIDVALIDITY") {
        values.push("UIDVALIDITY 1".to_string());
    }
    if values.is_empty() {
        values.push(format!("MESSAGES {}", messages));
    }

    format!("STATUS {} ({})", quote_mailbox(mailbox), values.join(" "))
}

fn next_uid(messages: &[Email]) -> i64 {
    messages.iter().map(|email| email.id).max().unwrap_or(0) + 1
}

fn first_unseen_sequence(messages: &[Email]) -> usize {
    messages
        .iter()
        .position(|email| !email.is_read)
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn unseen_count(messages: &[Email]) -> usize {
    messages.iter().filter(|email| !email.is_read).count()
}

fn canonical_mailbox_name(value: &str) -> Option<&'static str> {
    crate::imap::mailbox::standard_mailboxes()
        .iter()
        .copied()
        .find(|mailbox| mailbox.eq_ignore_ascii_case(value))
}

fn is_drafts_mailbox(value: &str) -> bool {
    value.eq_ignore_ascii_case(crate::imap::mailbox::DRAFTS)
}

fn is_valid_message_destination_mailbox(value: &str) -> bool {
    !is_drafts_mailbox(value)
}

fn quote_mailbox(value: &str) -> String {
    if value
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_')
    {
        value.to_string()
    } else {
        format!("\"{}\"", escape_imap_string(value))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imap_strings_are_escaped() {
        assert_eq!(escape_imap_string(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn recipient_envelope_handles_json_arrays() {
        let envelope = parse_recipients_envelope(r#"["User <user@example.com>"]"#);
        assert!(envelope.contains("user"));
        assert!(envelope.contains("example.com"));
    }

    #[test]
    fn status_response_includes_requested_items() {
        assert_eq!(
            build_status_response("INBOX", "(MESSAGES UNSEEN UIDNEXT UIDVALIDITY)", 3, 1, 9),
            "STATUS INBOX (MESSAGES 3 UNSEEN 1 UIDNEXT 9 UIDVALIDITY 1)"
        );
    }

    #[test]
    fn mailbox_unseen_helpers_use_sequence_and_count_correctly() {
        let mut messages = vec![
            test_email(1, true, false),
            test_email(2, false, true),
            test_email(3, false, false),
        ];

        assert_eq!(first_unseen_sequence(&messages), 2);
        assert_eq!(unseen_count(&messages), 2);

        messages[1].is_read = true;
        messages[2].is_read = true;
        assert_eq!(first_unseen_sequence(&messages), 0);
        assert_eq!(unseen_count(&messages), 0);
    }

    #[test]
    fn search_parser_handles_common_criteria() {
        assert_eq!(
            parse_search_criteria(r#"UNSEEN SUBJECT "Hello World" FROM sender@example.net"#),
            vec![
                SearchCriterion::Unseen,
                SearchCriterion::Subject("Hello World".to_string()),
                SearchCriterion::From("sender@example.net".to_string())
            ]
        );
        assert_eq!(
            parse_search_criteria(r#"NOT DELETED OR FROM a@example.com TO b@example.com"#),
            vec![
                SearchCriterion::Not(Box::new(SearchCriterion::Deleted)),
                SearchCriterion::Or(
                    Box::new(SearchCriterion::From("a@example.com".to_string())),
                    Box::new(SearchCriterion::To("b@example.com".to_string()))
                )
            ]
        );
    }

    #[test]
    fn expunge_sequence_numbers_account_for_prior_expunges() {
        let messages = vec![
            ImapMessage {
                sequence_number: 1,
                email: test_email(1, false, true),
            },
            ImapMessage {
                sequence_number: 2,
                email: test_email(2, false, true),
            },
            ImapMessage {
                sequence_number: 3,
                email: test_email(3, false, false),
            },
            ImapMessage {
                sequence_number: 4,
                email: test_email(4, false, true),
            },
        ];

        assert_eq!(expunge_sequence_numbers(&messages), vec![1, 1, 2]);
    }

    #[test]
    fn raw_message_sections_split_header_and_text() {
        let raw = b"From: a@example.com\r\nSubject: Hello\r\n\r\nBody\r\n";

        assert_eq!(
            raw_message_header(raw),
            b"From: a@example.com\r\nSubject: Hello\r\n".to_vec()
        );
        assert_eq!(raw_message_text(raw), b"Body\r\n".to_vec());
        assert_eq!(raw_message_text(b"Subject: only headers"), Vec::<u8>::new());
    }

    #[test]
    fn raw_message_header_fields_return_only_requested_headers() {
        let raw = b"From: a@example.com\r\nSubject: Hello\r\n folded\r\nDate: Wed, 10 Jun 2026 10:00:00 +0000\r\n\r\nBody";

        assert_eq!(
            raw_message_header_fields(raw, &["subject".to_string(), "date".to_string()]),
            b"Subject: Hello\r\n folded\r\nDate: Wed, 10 Jun 2026 10:00:00 +0000\r\n\r\n".to_vec()
        );
    }

    #[test]
    fn fetch_literal_detection_does_not_treat_rfc822_size_as_full_message() {
        let email = Email {
            id: 42,
            message_id: Some("<id@example.com>".to_string()),
            sender: "sender@example.com".to_string(),
            recipients: r#"["user@example.com"]"#.to_string(),
            subject: Some("Hello".to_string()),
            body_text: Some("Body".to_string()),
            body_html: None,
            raw_message: Some(b"Subject: Hello\r\n\r\nBody".to_vec()),
            dkim_signature: None,
            spf_result: None,
            dmarc_result: None,
            is_read: false,
            is_deleted: false,
            mailbox: Some("INBOX".to_string()),
            user_id: 1,
            created_at: None,
        };

        assert!(fetch_literals(&email, "RFC822.SIZE").is_empty());
        let literals = fetch_literals(&email, "RFC822");
        assert_eq!(literals.len(), 1);
        assert_eq!(literals[0].item, "RFC822");
    }

    #[test]
    fn imap_capabilities_hide_login_until_starttls() {
        assert_eq!(
            imap_capabilities(true),
            "IMAP4rev1 STARTTLS LOGINDISABLED MOVE"
        );
        assert_eq!(imap_capabilities(false), "IMAP4rev1 AUTH=PLAIN MOVE");
    }

    #[test]
    fn imap_starttls_available_requires_plain_starttls_support() {
        let mut config = Config::default();
        config.imap.enable_starttls = true;

        assert!(imap_starttls_available(&config, false, true));
        assert!(!imap_starttls_available(&config, true, true));
        assert!(!imap_starttls_available(&config, false, false));

        config.imap.enable_starttls = false;
        assert!(!imap_starttls_available(&config, false, true));
    }

    #[test]
    fn decodes_imap_authenticate_plain_response() {
        let credentials = decode_plain_auth_response("AHVzZXJAZXhhbXBsZS5jb20AcGFzcw==");
        assert_eq!(
            credentials,
            Some(("user@example.com".to_string(), "pass".to_string()))
        );
        assert_eq!(decode_plain_auth_response("not-base64"), None);
    }

    #[test]
    fn imap_auth_username_is_normalized() {
        assert_eq!(
            normalize_auth_username(" User@Example.COM "),
            "user@example.com"
        );
    }

    #[test]
    fn imap_auth_lockout_starts_after_session_failure_limit() {
        assert!(!imap_auth_locked_out(MAX_AUTH_FAILURES_PER_SESSION - 1));
        assert!(imap_auth_locked_out(MAX_AUTH_FAILURES_PER_SESSION));
    }

    #[tokio::test]
    async fn authenticate_plain_initial_response_logs_in() {
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

        let mut input = tokio::io::BufReader::new(
            b"a1 AUTHENTICATE PLAIN AHVzZXJAZXhhbXBsZS5jb20AcGFzcw==\r\na2 LOGOUT\r\n".as_slice(),
        );
        let mut output = Vec::new();
        let result = handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db,
            tls_imap_options(),
        )
        .await
        .expect("session");

        assert!(matches!(result, ImapSessionResult::Done));
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("a1 OK AUTHENTICATE completed"));
    }

    #[tokio::test]
    async fn authenticate_plain_normalizes_username() {
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

        let mut input = tokio::io::BufReader::new(
            b"a1 AUTHENTICATE PLAIN ACBVc2VyQEV4YW1wbGUuQ09NICAAcGFzcw==\r\na2 LOGOUT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();
        let result = handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db,
            tls_imap_options(),
        )
        .await
        .expect("session");

        assert!(matches!(result, ImapSessionResult::Done));
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("a1 OK AUTHENTICATE completed"));
    }

    #[tokio::test]
    async fn move_command_moves_messages_between_mailboxes() {
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
        let user = queries::create_user(&db, "user@example.com", &hash, domain.id, false)
            .await
            .expect("user");
        queries::save_email(
            &db,
            test_new_email(
                user.id,
                Some("<move@example.com>"),
                "sender@example.net",
                Some("move me"),
                Some("body"),
                Some(b"Subject: move me\r\n\r\nbody"),
            ),
        )
        .await
        .expect("email");

        let mut input = tokio::io::BufReader::new(
            b"a1 LOGIN user@example.com pass\r\na2 SELECT INBOX\r\na3 MOVE 1 Trash\r\na4 LOGOUT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();
        let result = handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db.clone(),
            tls_imap_options(),
        )
        .await
        .expect("session");

        assert!(matches!(result, ImapSessionResult::Done));
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("a3 OK MOVE completed"));
        assert!(
            queries::get_emails_for_imap(&db, user.id, "INBOX")
                .await
                .expect("inbox")
                .is_empty()
        );
        assert_eq!(
            queries::get_emails_for_imap(&db, user.id, "Trash")
                .await
                .expect("trash")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn copy_command_rejects_drafts_destination() {
        let (db, user_id) = setup_imap_user().await;
        queries::save_email(
            &db,
            test_new_email(
                user_id,
                Some("<copy-draft@example.com>"),
                "sender@example.net",
                Some("copy draft"),
                Some("body"),
                Some(b"Subject: copy draft\r\n\r\nbody"),
            ),
        )
        .await
        .expect("email");

        let mut input = tokio::io::BufReader::new(
            b"a1 LOGIN user@example.com pass\r\na2 SELECT INBOX\r\na3 COPY 1 Drafts\r\na4 LOGOUT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();
        handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db.clone(),
            tls_imap_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("a3 NO Drafts cannot be used as destination"));
        assert_eq!(
            queries::get_emails_for_imap(&db, user_id, "INBOX")
                .await
                .expect("inbox")
                .len(),
            1
        );
        assert!(
            queries::get_emails_for_imap(&db, user_id, "Drafts")
                .await
                .expect("drafts")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn move_command_rejects_selected_draft_messages() {
        let (db, user_id) = setup_imap_user().await;
        queries::save_email(
            &db,
            queries::NewEmail {
                message_id: Some("<move-draft@example.com>"),
                sender: "sender@example.net",
                recipients: r#"["user@example.com"]"#,
                subject: Some("draft"),
                body_text: Some("body"),
                body_html: None,
                raw_message: Some(b"Subject: draft\r\n\r\nbody"),
                user_id,
                mailbox: "Drafts",
                is_read: true,
            },
        )
        .await
        .expect("draft");

        let mut input = tokio::io::BufReader::new(
            b"a1 LOGIN user@example.com pass\r\na2 SELECT Drafts\r\na3 MOVE 1 Trash\r\na4 LOGOUT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();
        handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db.clone(),
            tls_imap_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("a3 NO Draft messages cannot be moved"));
        assert_eq!(
            queries::get_emails_for_imap(&db, user_id, "Drafts")
                .await
                .expect("drafts")
                .len(),
            1
        );
        assert!(
            queries::get_emails_for_imap(&db, user_id, "Trash")
                .await
                .expect("trash")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn deleted_messages_remain_until_expunge() {
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
        let user = queries::create_user(&db, "user@example.com", &hash, domain.id, false)
            .await
            .expect("user");
        queries::save_email(
            &db,
            test_new_email(
                user.id,
                Some("<delete@example.com>"),
                "sender@example.net",
                Some("delete me"),
                Some("body"),
                Some(b"Subject: delete me\r\n\r\nbody"),
            ),
        )
        .await
        .expect("email");

        let mut input = tokio::io::BufReader::new(
            b"a1 LOGIN user@example.com pass\r\na2 SELECT INBOX\r\na3 STORE 1 +FLAGS (\\Deleted)\r\na4 SELECT INBOX\r\na5 EXPUNGE\r\na6 SELECT INBOX\r\na7 LOGOUT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();
        handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db.clone(),
            tls_imap_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("* 1 FETCH (FLAGS (\\Deleted))"));
        assert!(output.contains("a3 OK STORE completed"));
        assert!(output.contains("* 1 EXISTS"));
        assert!(output.contains("* 1 EXPUNGE"));
        assert!(output.contains("* 0 EXISTS"));
        assert!(
            queries::get_emails_for_imap(&db, user.id, "INBOX")
                .await
                .expect("inbox")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn select_rejects_unknown_mailbox() {
        let (db, _) = setup_imap_user().await;

        let mut input = tokio::io::BufReader::new(
            b"a1 LOGIN user@example.com pass\r\na2 SELECT Archive\r\na3 FETCH 1 FLAGS\r\na4 LOGOUT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();
        handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db,
            tls_imap_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("a2 NO Unknown mailbox"));
        assert!(output.contains("a3 NO Not selected"));
    }

    #[tokio::test]
    async fn search_handles_common_criteria() {
        let (db, user_id) = setup_imap_user().await;
        let first = queries::save_email(
            &db,
            test_new_email(
                user_id,
                Some("<search-1@example.com>"),
                "Alice <alice@example.net>",
                Some("Quarterly Report"),
                Some("Needle in the body"),
                Some(
                    b"From: Alice <alice@example.net>\r\nTo: user@example.com\r\nSubject: Quarterly Report\r\n\r\nNeedle in the body",
                ),
            ),
        )
        .await
        .expect("first email");
        let second = queries::save_email(
            &db,
            test_new_email(
                user_id,
                Some("<search-2@example.com>"),
                "Bob <bob@example.net>",
                Some("Different Subject"),
                Some("Needle in a read message"),
                Some(
                    b"From: Bob <bob@example.net>\r\nTo: user@example.com\r\nSubject: Different Subject\r\n\r\nNeedle in a read message",
                ),
            ),
        )
        .await
        .expect("second email");
        let third = queries::save_email(
            &db,
            test_new_email(
                user_id,
                Some("<search-3@example.com>"),
                "Carol <carol@example.net>",
                Some("Deleted Subject"),
                Some("Deleted body"),
                Some(
                    b"From: Carol <carol@example.net>\r\nTo: user@example.com\r\nSubject: Deleted Subject\r\n\r\nDeleted body",
                ),
            ),
        )
        .await
        .expect("third email");
        queries::set_email_read(&db, second.id, true)
            .await
            .expect("mark read");
        queries::set_email_deleted(&db, third.id, true)
            .await
            .expect("mark deleted");

        let commands = format!(
            "a1 LOGIN user@example.com pass\r\n\
             a2 SELECT INBOX\r\n\
             a3 SEARCH SUBJECT \"Quarterly Report\"\r\n\
             a4 SEARCH FROM alice@example.net UNSEEN\r\n\
             a5 SEARCH TEXT Needle UNDELETED\r\n\
             a6 SEARCH NOT DELETED\r\n\
             a7 SEARCH DELETED\r\n\
             a8 UID SEARCH UID {} SUBJECT \"Quarterly Report\"\r\n\
             a9 LOGOUT\r\n",
            first.id
        );
        let mut input = tokio::io::BufReader::new(commands.as_bytes());
        let mut output = Vec::new();
        handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db,
            tls_imap_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("* SEARCH 1\r\na3 OK SEARCH completed"));
        assert!(output.contains("* SEARCH 1\r\na4 OK SEARCH completed"));
        assert!(output.contains("* SEARCH 1 2\r\na5 OK SEARCH completed"));
        assert!(output.contains("* SEARCH 1 2\r\na6 OK SEARCH completed"));
        assert!(output.contains("* SEARCH 3\r\na7 OK SEARCH completed"));
        assert!(output.contains(&format!(
            "* SEARCH {}\r\na8 OK UID SEARCH completed",
            first.id
        )));
    }

    #[tokio::test]
    async fn fetch_can_return_header_and_text_sections() {
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
        let user = queries::create_user(&db, "user@example.com", &hash, domain.id, false)
            .await
            .expect("user");
        queries::save_email(
            &db,
            test_new_email(
                user.id,
                Some("<fetch@example.com>"),
                "sender@example.net",
                Some("Fetch me"),
                Some("Body line"),
                Some(b"From: sender@example.net\r\nSubject: Fetch me\r\n\r\nBody line\r\n"),
            ),
        )
        .await
        .expect("email");

        let mut input = tokio::io::BufReader::new(
            b"a1 LOGIN user@example.com pass\r\na2 SELECT INBOX\r\na3 FETCH 1 (BODY.PEEK[HEADER] BODY.PEEK[TEXT] BODY.PEEK[HEADER.FIELDS (FROM SUBJECT)])\r\na4 LOGOUT\r\n"
                .as_slice(),
        );
        let mut output = Vec::new();
        handle_imap_session(
            &mut input,
            &mut output,
            Arc::new(Config::default()),
            db,
            tls_imap_options(),
        )
        .await
        .expect("session");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("BODY[HEADER] {45}"));
        assert!(output.contains("From: sender@example.net\r\nSubject: Fetch me\r\n"));
        assert!(output.contains("BODY[TEXT] {11}"));
        assert!(output.contains("Body line\r\n"));
        assert!(output.contains("BODY[HEADER.FIELDS (FROM SUBJECT)] {47}"));
    }

    async fn setup_imap_user() -> (sqlx::SqlitePool, i64) {
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
        let user = queries::create_user(&db, "user@example.com", &hash, domain.id, false)
            .await
            .expect("user");

        (db, user.id)
    }

    fn tls_imap_options() -> ImapSessionOptions {
        ImapSessionOptions::new("127.0.0.1:143", true, false, false)
    }

    fn test_new_email<'a>(
        user_id: i64,
        message_id: Option<&'a str>,
        sender: &'a str,
        subject: Option<&'a str>,
        body_text: Option<&'a str>,
        raw_message: Option<&'a [u8]>,
    ) -> queries::NewEmail<'a> {
        queries::NewEmail {
            message_id,
            sender,
            recipients: r#"["user@example.com"]"#,
            subject,
            body_text,
            body_html: None,
            raw_message,
            user_id,
            mailbox: "INBOX",
            is_read: false,
        }
    }

    fn test_email(id: i64, is_read: bool, is_deleted: bool) -> Email {
        Email {
            id,
            message_id: Some(format!("<{}@example.com>", id)),
            sender: "sender@example.com".to_string(),
            recipients: r#"["user@example.com"]"#.to_string(),
            subject: Some("subject".to_string()),
            body_text: Some("body".to_string()),
            body_html: None,
            raw_message: Some(b"Subject: subject\r\n\r\nbody".to_vec()),
            dkim_signature: None,
            spf_result: None,
            dmarc_result: None,
            is_read,
            is_deleted,
            mailbox: Some("INBOX".to_string()),
            user_id: 1,
            created_at: None,
        }
    }
}
