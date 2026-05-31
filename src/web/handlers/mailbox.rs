use axum::{
    extract::{Path, Query, State, Extension},
    Json,
};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use crate::db::queries;
use crate::db::models::SendEmailRequest;
use crate::mail::delivery::MailDelivery;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

#[derive(Deserialize)]
pub struct ListParams {
    pub mailbox: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

pub async fn list_emails(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mailbox = params.mailbox.unwrap_or_else(|| "INBOX".to_string());
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(100);
    let offset = (page - 1) * per_page;

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
    let mailbox = body["mailbox"]
        .as_str()
        .ok_or(StatusCode::BAD_REQUEST)?;

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

    delivery
        .send_email(
            &claims.email,
            &payload.to,
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
