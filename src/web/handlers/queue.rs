use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
};
use serde::Deserialize;
use serde_json::json;

use crate::db::models::OutboundQueueItem;
use crate::db::queries;
use crate::web::middleware::Claims;
use crate::web::{response, router::AppState};

#[derive(Deserialize)]
pub struct QueueParams {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

fn queue_item_json(item: &OutboundQueueItem) -> serde_json::Value {
    json!({
        "id": item.id,
        "envelope_sender": item.envelope_sender,
        "recipients": queries::outbound_recipients(item),
        "attempts": item.attempts,
        "max_attempts": item.max_attempts,
        "status": item.status,
        "last_error": item.last_error,
        "next_attempt_at": item.next_attempt_at,
        "created_at": item.created_at,
        "updated_at": item.updated_at,
        "raw_size": item.raw_message.len(),
    })
}

pub async fn list_queue(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<QueueParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let status = params.status.as_deref().filter(|status| {
        matches!(
            *status,
            queries::OUTBOUND_STATUS_QUEUED
                | queries::OUTBOUND_STATUS_SENT
                | queries::OUTBOUND_STATUS_FAILED
        )
    });
    let items = queries::list_outbound_queue_items(&state.db, status, params.limit.unwrap_or(50))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items = items.iter().map(queue_item_json).collect::<Vec<_>>();

    Ok(Json(json!({ "items": items })))
}

pub async fn retry_queue_item(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let Some(item) = queries::retry_outbound_queue_item(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(Json(json!({ "item": queue_item_json(&item) })))
}

pub async fn delete_queue_item(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let deleted = queries::delete_outbound_queue_item(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !deleted {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(response::ok().1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_item_response_omits_raw_message() {
        let item = OutboundQueueItem {
            id: 1,
            envelope_sender: "sender@example.com".to_string(),
            recipients: r#"["a@example.net"]"#.to_string(),
            raw_message: b"raw message".to_vec(),
            attempts: 2,
            max_attempts: 5,
            status: queries::OUTBOUND_STATUS_FAILED.to_string(),
            last_error: Some("failed".to_string()),
            next_attempt_at: None,
            created_at: None,
            updated_at: None,
        };

        let value = queue_item_json(&item);
        assert_eq!(value["raw_size"], 11);
        assert!(value.get("raw_message").is_none());
        assert_eq!(value["recipients"][0], "a@example.net");
    }
}
