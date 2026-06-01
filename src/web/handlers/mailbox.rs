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
use crate::mail::delivery::MailDelivery;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

#[derive(Deserialize)]
pub struct ListParams {
    pub mailbox: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
}

pub async fn list_emails(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(100);
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
    let delivery = MailDelivery::new(state.config.clone(), state.db.clone());

    // Combine to, cc, bcc for sending
    let mut all_recipients = payload.to.clone();
    if let Some(ref cc) = payload.cc {
        all_recipients.extend(cc.clone());
    }
    if let Some(ref bcc) = payload.bcc {
        all_recipients.extend(bcc.clone());
    }

    delivery
        .send_email(
            &claims.email,
            &all_recipients,
            &payload.subject,
            payload.body_text.as_deref(),
            payload.body_html.as_deref(),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Also save to Sent mailbox
    let recipients_json = serde_json::to_string(&payload.to).unwrap_or_default();
    let _ = queries::save_email(
        &state.db,
        None,
        &claims.email,
        &recipients_json,
        Some(&payload.subject),
        payload.body_text.as_deref(),
        payload.body_html.as_deref(),
        None,
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
