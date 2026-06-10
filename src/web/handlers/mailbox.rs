use axum::http::{StatusCode, header};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;

use crate::db::models::SendEmailRequest;
use crate::db::queries;
use crate::mail::delivery::{ComposedEmail, MailDelivery};
use crate::web::middleware::Claims;
use crate::web::router::AppState;

#[derive(Deserialize)]
pub struct ListParams {
    pub mailbox: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
}

fn is_valid_mailbox_name(value: &str) -> bool {
    matches!(value, "INBOX" | "Sent" | "Drafts" | "Trash" | "Spam")
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
        .filter(|ch| !matches!(ch, '"' | '\\' | '\r' | '\n'))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "attachment".to_string()
    } else {
        cleaned.to_string()
    }
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

fn visible_recipients_json(to: &[String], cc: Option<&[String]>) -> String {
    let mut visible = Vec::new();
    extend_unique_recipients(&mut visible, to);
    if let Some(cc) = cc {
        extend_unique_recipients(&mut visible, cc);
    }
    serde_json::to_string(&visible).unwrap_or_default()
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
            "emails": emails,
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
        "emails": emails,
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
    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Ensure the email belongs to the authenticated user
    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }

    // Mark as read
    let _ = queries::mark_email_read(&state.db, id).await;

    let attachments = queries::get_attachments_by_email(&state.db, id)
        .await
        .unwrap_or_default();

    Ok(Json(json!({
        "email": email,
        "attachments": attachments,
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

pub async fn move_email(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mailbox = body["mailbox"].as_str().ok_or(StatusCode::BAD_REQUEST)?;
    if !is_valid_mailbox_name(mailbox) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let email = queries::get_email_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if email.user_id != claims.sub {
        return Err(StatusCode::FORBIDDEN);
    }

    queries::move_email(&state.db, id, mailbox)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn send_email(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SendEmailRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if payload.to.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let sender = queries::get_user_by_id(&state.db, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if sender.email != claims.email {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let delivery = MailDelivery::new(state.config.clone(), state.db.clone());

    // Combine to, cc, bcc for validation and envelope delivery.
    let mut all_recipients = payload.to.clone();
    if let Some(ref cc) = payload.cc {
        all_recipients.extend(cc.clone());
    }
    if let Some(ref bcc) = payload.bcc {
        all_recipients.extend(bcc.clone());
    }
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
            to: &payload.to,
            cc: payload.cc.as_deref().unwrap_or(&[]),
            bcc: payload.bcc.as_deref().unwrap_or(&[]),
            subject: &payload.subject,
            body_text: payload.body_text.as_deref(),
            body_html: payload.body_html.as_deref(),
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Also save to Sent mailbox
    let recipients_json = visible_recipients_json(&payload.to, payload.cc.as_deref());
    let _ = queries::save_email(
        &state.db,
        None,
        &claims.email,
        &recipients_json,
        Some(&payload.subject),
        payload.body_text.as_deref(),
        payload.body_html.as_deref(),
        Some(&sent_raw_message),
        claims.sub,
        "Sent",
    )
    .await;

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
    fn attachment_filenames_are_header_safe() {
        assert_eq!(sanitize_attachment_filename(" report.pdf "), "report.pdf");
        assert_eq!(sanitize_attachment_filename("\"bad\r\n.txt"), "bad.txt");
        assert_eq!(sanitize_attachment_filename("\r\n"), "attachment");
    }

    #[test]
    fn sent_visible_recipients_include_cc_but_not_duplicates() {
        let to = vec!["a@example.com".to_string()];
        let cc = vec!["A@example.com".to_string(), "b@example.com".to_string()];
        assert_eq!(
            visible_recipients_json(&to, Some(&cc)),
            r#"["a@example.com","b@example.com"]"#
        );
    }
}
