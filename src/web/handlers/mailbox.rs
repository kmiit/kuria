use axum::http::{StatusCode, header};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    response::IntoResponse,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;

use crate::db::models::{Email, SendEmailRequest};
use crate::db::queries;
use crate::mail::compose::{ComposedAttachment, save_composed_attachments};
use crate::mail::delivery::{ComposedEmail, MailDelivery};
use crate::web::middleware::Claims;
use crate::web::router::AppState;

const MAX_ATTACHMENTS: usize = 10;
const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

#[derive(Deserialize)]
pub struct ListParams {
    pub mailbox: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveDraftRequest {
    pub id: Option<i64>,
    pub to: Option<Vec<String>>,
    pub cc: Option<Vec<String>>,
    pub bcc: Option<Vec<String>>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub send_as_html: Option<bool>,
    pub attachments: Option<Vec<crate::db::models::SendEmailAttachmentRequest>>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct StoredDraft {
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body_text: Option<String>,
    body_html: Option<String>,
    send_as_html: bool,
}

fn is_valid_mailbox_name(value: &str) -> bool {
    matches!(value, "INBOX" | "Sent" | "Drafts" | "Trash" | "Spam")
}

fn is_valid_move_target_mailbox(value: &str) -> bool {
    is_valid_mailbox_name(value) && value != "Drafts"
}

fn is_valid_email_address(value: &str) -> bool {
    let Some((local, domain)) = value.trim().split_once('@') else {
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

fn sanitize_attachment_filename(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\\' | '/' | ':' | '\r' | '\n') && !ch.is_control())
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "attachment".to_string()
    } else {
        cleaned.to_string()
    }
}

fn normalize_content_type(value: Option<&str>) -> String {
    let value = value.unwrap_or("application/octet-stream").trim();
    if is_valid_content_type(value) {
        value.to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

fn is_valid_content_type(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 || value.contains(char::is_whitespace) {
        return false;
    }
    let Some((top, sub)) = value.split_once('/') else {
        return false;
    };
    !top.is_empty()
        && !sub.is_empty()
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'/' | b'-' | b'+' | b'.'))
}

fn decode_attachment_data(value: &str) -> Result<Vec<u8>, StatusCode> {
    let encoded = value
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or(value)
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| StatusCode::BAD_REQUEST)
}

fn decode_request_attachments(
    attachments: Option<Vec<crate::db::models::SendEmailAttachmentRequest>>,
) -> Result<Vec<ComposedAttachment>, StatusCode> {
    let Some(attachments) = attachments else {
        return Ok(Vec::new());
    };
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let mut total_size = 0usize;
    let mut decoded = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let filename = sanitize_attachment_filename(&attachment.filename);
        let content_type = normalize_content_type(attachment.content_type.as_deref());
        let data = decode_attachment_data(&attachment.data_base64)?;
        if data.is_empty() || data.len() > MAX_ATTACHMENT_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        total_size = total_size
            .checked_add(data.len())
            .ok_or(StatusCode::PAYLOAD_TOO_LARGE)?;
        if total_size > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }

        decoded.push(ComposedAttachment {
            filename,
            content_type,
            data,
        });
    }

    Ok(decoded)
}

fn stored_attachments_to_composed(
    attachments: Vec<crate::db::models::Attachment>,
) -> Vec<ComposedAttachment> {
    attachments
        .into_iter()
        .filter_map(|attachment| {
            let data = attachment.data?;
            if data.is_empty() {
                return None;
            }

            Some(ComposedAttachment {
                filename: sanitize_attachment_filename(
                    attachment.filename.as_deref().unwrap_or("attachment"),
                ),
                content_type: normalize_content_type(attachment.content_type.as_deref()),
                data,
            })
        })
        .collect()
}

fn attachment_metadata_json(attachments: Vec<crate::db::models::Attachment>) -> serde_json::Value {
    let attachments = attachments
        .into_iter()
        .map(|attachment| {
            json!({
                "id": attachment.id,
                "email_id": attachment.email_id,
                "filename": attachment.filename,
                "content_type": attachment.content_type,
                "size": attachment.size,
            })
        })
        .collect::<Vec<_>>();

    json!(attachments)
}

fn extract_email_address(value: &str) -> String {
    let value = value.trim();
    if let Some(start) = value.rfind('<')
        && let Some(end) = value[start + 1..].find('>')
    {
        return value[start + 1..start + 1 + end].trim().to_string();
    }

    value.trim_matches('"').trim().to_string()
}

fn clean_recipient_list(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| extract_email_address(&value))
        .filter(|value| !value.is_empty())
        .collect()
}

fn clean_optional_recipient_list(values: Option<Vec<String>>) -> Vec<String> {
    clean_recipient_list(values.unwrap_or_default())
}

fn extend_unique_recipients(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(value))
        {
            target.push(value.clone());
        }
    }
}

fn recipients_json(groups: &[&[String]]) -> String {
    let mut recipients = Vec::new();
    for group in groups {
        extend_unique_recipients(&mut recipients, group);
    }
    serde_json::to_string(&recipients).unwrap_or_default()
}

fn sender_recipients_json(to: &[String], cc: &[String], bcc: &[String]) -> String {
    recipients_json(&[to, cc, bcc])
}

fn draft_recipients_json(draft: &StoredDraft) -> String {
    recipients_json(&[&draft.to, &draft.cc, &draft.bcc])
}

fn recipients_json_from_email(email: &Email) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&email.recipients).unwrap_or_default()
}

fn email_summary_json(email: crate::db::models::EmailSummary) -> serde_json::Value {
    json!({
        "id": email.id,
        "sender": email.sender,
        "recipients": email.recipients,
        "subject": email.subject,
        "body_text": email.body_text,
        "is_read": email.is_read,
        "mailbox": email.mailbox,
        "created_at": email.created_at,
        "attachment_count": email.attachment_count,
        "has_attachments": email.attachment_count > 0,
    })
}

fn email_summaries_json(emails: Vec<crate::db::models::EmailSummary>) -> serde_json::Value {
    json!(
        emails
            .into_iter()
            .map(email_summary_json)
            .collect::<Vec<_>>()
    )
}

fn draft_from_request(payload: SaveDraftRequest) -> StoredDraft {
    let send_as_html = payload.send_as_html.unwrap_or_else(|| {
        payload
            .body_html
            .as_deref()
            .is_some_and(|body| !body.is_empty())
    });
    let body = if send_as_html {
        payload.body_html.unwrap_or_default()
    } else {
        payload.body_text.unwrap_or_default()
    };

    StoredDraft {
        to: clean_optional_recipient_list(payload.to),
        cc: clean_optional_recipient_list(payload.cc),
        bcc: clean_optional_recipient_list(payload.bcc),
        subject: payload.subject.unwrap_or_default(),
        body_text: (!send_as_html).then_some(body.clone()),
        body_html: send_as_html.then_some(body),
        send_as_html,
    }
}

fn draft_has_content(draft: &StoredDraft) -> bool {
    !draft.to.is_empty()
        || !draft.cc.is_empty()
        || !draft.bcc.is_empty()
        || !draft.subject.trim().is_empty()
        || draft
            .body_text
            .as_deref()
            .is_some_and(|body| !body.trim().is_empty())
        || draft
            .body_html
            .as_deref()
            .is_some_and(|body| !body.trim().is_empty())
}

fn decode_stored_draft(email: &Email) -> StoredDraft {
    if let Some(raw) = email.raw_message.as_deref()
        && let Ok(draft) = serde_json::from_slice::<StoredDraft>(raw)
    {
        return draft;
    }

    StoredDraft {
        to: recipients_json_from_email(email),
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: email.subject.clone().unwrap_or_default(),
        body_text: email.body_text.clone(),
        body_html: email.body_html.clone(),
        send_as_html: email.body_html.is_some(),
    }
}

fn draft_json(email: &Email) -> serde_json::Value {
    let draft = decode_stored_draft(email);
    json!({
        "id": email.id,
        "to": draft.to,
        "cc": draft.cc,
        "bcc": draft.bcc,
        "subject": draft.subject,
        "body_text": draft.body_text,
        "body_html": draft.body_html,
        "send_as_html": draft.send_as_html,
        "created_at": email.created_at,
    })
}

pub async fn list_emails(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).clamp(1, 100);
    let offset = (page - 1) * per_page;

    // If search query is provided, search across all mailboxes
    if let Some(ref query) = params.search
        && !query.is_empty()
    {
        let emails = queries::search_emails(&state.db, claims.sub, query, per_page, offset)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let total = queries::count_search_emails(&state.db, claims.sub, query)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        return Ok(Json(json!({
            "emails": email_summaries_json(emails),
            "total": total,
            "page": page,
            "per_page": per_page,
        })));
    }

    let mailbox = params.mailbox.unwrap_or_else(|| "INBOX".to_string());
    if !is_valid_mailbox_name(&mailbox) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let emails = queries::get_emails_by_user(&state.db, claims.sub, &mailbox, per_page, offset)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = queries::count_emails_by_user(&state.db, claims.sub, &mailbox)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "emails": email_summaries_json(emails),
        "total": total,
        "page": page,
        "per_page": per_page,
    })))
}

pub async fn get_email(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Ensure the email belongs to the authenticated user
    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }

    // Mark as read
    let _ = queries::mark_email_read(&state.db, id).await;
    email.is_read = true;

    let attachments = queries::get_attachments_by_email(&state.db, id)
        .await
        .unwrap_or_default();

    Ok(Json(json!({
        "email": email,
        "attachments": attachment_metadata_json(attachments),
    })))
}

pub async fn delete_email(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }

    queries::delete_email(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn empty_trash(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let deleted = queries::empty_trash(&state.db, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true, "deleted": deleted })))
}

pub async fn mark_read(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }

    queries::mark_email_read(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn mark_unread(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }

    queries::set_email_read(&state.db, id, false)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn move_email(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mailbox = body["mailbox"].as_str().ok_or(StatusCode::BAD_REQUEST)?;
    if !is_valid_move_target_mailbox(mailbox) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }
    if email.mailbox.as_deref() == Some("Drafts") {
        return Err(StatusCode::BAD_REQUEST);
    }

    queries::move_email(&state.db, id, mailbox)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn get_draft(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }
    if email.mailbox.as_deref() != Some("Drafts") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let attachments = queries::get_attachments_by_email(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "draft": draft_json(&email),
        "email": email,
        "attachments": attachment_metadata_json(attachments),
    })))
}

pub async fn save_draft(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SaveDraftRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut payload = payload;
    let draft_id = payload.id;
    let attachments = decode_request_attachments(payload.attachments.take())?;
    let draft = draft_from_request(payload);
    if !draft_has_content(&draft) && attachments.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let raw_message = serde_json::to_vec(&draft).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let recipients_json = draft_recipients_json(&draft);
    let email = queries::save_draft(
        &state.db,
        draft_id,
        queries::DraftEmail {
            sender: &claims.email,
            recipients: &recipients_json,
            subject: Some(&draft.subject),
            body_text: draft.body_text.as_deref(),
            body_html: draft.body_html.as_deref(),
            raw_message: &raw_message,
            user_id: claims.sub,
        },
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    queries::replace_attachments(&state.db, email.id, &attachments)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let saved_attachments = queries::get_attachments_by_email(&state.db, email.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "ok": true,
        "draft": draft_json(&email),
        "email": email,
        "attachments": attachment_metadata_json(saved_attachments),
    })))
}

pub async fn delete_draft(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }
    if email.mailbox.as_deref() != Some("Drafts") {
        return Err(StatusCode::BAD_REQUEST);
    }

    queries::permanently_delete_email(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn send_email(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SendEmailRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let draft_id = payload.draft_id;
    let request_attachments = payload.attachments;
    let to = clean_recipient_list(payload.to);
    let cc = clean_optional_recipient_list(payload.cc);
    let bcc = clean_optional_recipient_list(payload.bcc);

    let sender = queries::get_user_by_id(&state.db, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if sender.email != claims.email {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if let Some(draft_id) = draft_id {
        let draft = queries::get_email_by_id(&state.db, draft_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?;
        if draft.user_id != claims.sub {
            return Err(StatusCode::FORBIDDEN);
        }
        if draft.mailbox.as_deref() != Some("Drafts") {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let delivery = MailDelivery::with_plugins(
        state.config.clone(),
        state.db.clone(),
        state.plugins.clone(),
    );
    let attachments = if request_attachments.is_none() {
        if let Some(draft_id) = draft_id {
            let draft_attachments = queries::get_attachments_by_email(&state.db, draft_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            stored_attachments_to_composed(draft_attachments)
        } else {
            Vec::new()
        }
    } else {
        decode_request_attachments(request_attachments)?
    };

    // Combine to, cc, bcc for validation and envelope delivery.
    let mut all_recipients = to.clone();
    all_recipients.extend(cc.clone());
    all_recipients.extend(bcc.clone());
    if all_recipients.is_empty()
        || all_recipients
            .iter()
            .any(|recipient| !is_valid_email_address(recipient))
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let sent_raw_message = delivery
        .send_composed_email(ComposedEmail {
            from: &claims.email,
            to: &to,
            cc: &cc,
            bcc: &bcc,
            subject: &payload.subject,
            body_text: payload.body_text.as_deref(),
            body_html: payload.body_html.as_deref(),
            attachments: &attachments,
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Also save to Sent mailbox
    let recipients_json = sender_recipients_json(&to, &cc, &bcc);
    match queries::save_email(
        &state.db,
        queries::NewEmail {
            message_id: None,
            sender: &claims.email,
            recipients: &recipients_json,
            subject: Some(&payload.subject),
            body_text: payload.body_text.as_deref(),
            body_html: payload.body_html.as_deref(),
            raw_message: Some(&sent_raw_message),
            user_id: claims.sub,
            mailbox: "Sent",
            is_read: true,
        },
    )
    .await
    {
        Ok(sent_email) => {
            if let Err(error) =
                save_composed_attachments(&state.db, sent_email.id, &attachments).await
            {
                tracing::warn!(
                    "Sent email but failed to save sent attachments for user {}: {}",
                    claims.sub,
                    error
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                "Sent email but failed to save Sent copy for user {}: {}",
                claims.sub,
                error
            );
        }
    }

    if let Some(draft_id) = draft_id
        && let Err(error) = queries::permanently_delete_email(&state.db, draft_id).await
    {
        tracing::warn!(
            "Sent email but failed to delete draft {} for user {}: {}",
            draft_id,
            claims.sub,
            error
        );
    }

    Ok(Json(json!({ "ok": true })))
}

pub async fn download_attachment(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(attachment_id): Path<i64>,
) -> Result<impl IntoResponse, StatusCode> {
    let attachment = queries::get_attachment_by_id(&state.db, attachment_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Verify the email belongs to the user
    let email = queries::get_email_by_id(&state.db, attachment.email_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }

    let filename = attachment
        .filename
        .unwrap_or_else(|| "attachment".to_string());
    let filename = sanitize_attachment_filename(&filename);
    let content_type = attachment
        .content_type
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let data = attachment.data.unwrap_or_default();

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        data,
    ))
}

pub async fn get_mailbox_counts(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let counts = queries::get_mailbox_counts(&state.db, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut mailbox_counts = serde_json::Map::new();
    for (name, total, unread) in counts {
        mailbox_counts.insert(name, json!({ "total": total, "unread": unread }));
    }

    Ok(Json(json!({ "mailboxes": mailbox_counts })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_names_are_limited_to_known_folders() {
        assert!(is_valid_mailbox_name("INBOX"));
        assert!(is_valid_mailbox_name("Trash"));
        assert!(!is_valid_mailbox_name("../INBOX"));
        assert!(!is_valid_mailbox_name("Archive"));
    }

    #[test]
    fn move_targets_exclude_drafts() {
        assert!(is_valid_move_target_mailbox("INBOX"));
        assert!(is_valid_move_target_mailbox("Trash"));
        assert!(!is_valid_move_target_mailbox("Drafts"));
        assert!(!is_valid_move_target_mailbox("Archive"));
    }

    #[test]
    fn attachment_filenames_are_header_safe() {
        assert_eq!(sanitize_attachment_filename(" report.pdf "), "report.pdf");
        assert_eq!(sanitize_attachment_filename("\"bad\r\n.txt"), "bad.txt");
        assert_eq!(sanitize_attachment_filename("\r\n"), "attachment");
    }

    #[test]
    fn attachment_metadata_omits_binary_data() {
        let metadata = attachment_metadata_json(vec![crate::db::models::Attachment {
            id: 1,
            email_id: 2,
            filename: Some("report.pdf".to_string()),
            content_type: Some("application/pdf".to_string()),
            data: Some(vec![1, 2, 3]),
            size: Some(3),
        }]);

        assert_eq!(metadata[0]["id"], 1);
        assert_eq!(metadata[0]["filename"], "report.pdf");
        assert!(metadata[0].get("data").is_none());
    }

    #[test]
    fn sender_recipients_include_cc_and_bcc_but_not_duplicates() {
        let to = vec!["a@example.com".to_string()];
        let cc = vec!["A@example.com".to_string(), "b@example.com".to_string()];
        let bcc = vec!["hidden@example.com".to_string()];
        assert_eq!(
            sender_recipients_json(&to, &cc, &bcc),
            r#"["a@example.com","b@example.com","hidden@example.com"]"#
        );
    }

    #[test]
    fn recipient_cleanup_extracts_angle_addresses() {
        assert_eq!(
            clean_recipient_list([
                "Display Name <User@Example.COM>".to_string(),
                " plain@example.net ".to_string(),
                "".to_string(),
            ]),
            vec![
                "User@Example.COM".to_string(),
                "plain@example.net".to_string()
            ]
        );
        assert!(is_valid_email_address(&extract_email_address(
            "\"User\" <user@example.com>"
        )));
    }
}
