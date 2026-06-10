use axum::http::StatusCode;
use axum::{Json, extract::{Extension, Path, State}};
use serde_json::json;

use crate::db::{api_token_queries, models::CreateApiTokenRequest, queries};
use crate::web::middleware::Claims;
use crate::web::router::AppState;
use crate::web::response;

fn generate_api_token() -> String {
    use rand_core::{OsRng, RngCore};
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("krt_{}", base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes))
}

pub async fn create_token(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateApiTokenRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let user = queries::get_user_by_id(&state.db, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if !user.api_enabled {
        return Err(StatusCode::FORBIDDEN);
    }

    if payload.name.trim().is_empty() || payload.name.len() > 100 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let token = generate_api_token();
    let api_token = api_token_queries::create_api_token(&state.db, claims.sub, &token, &payload.name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "id": api_token.id,
        "name": api_token.name,
        "token": token,
        "created_at": api_token.created_at,
    })))
}

pub async fn list_tokens(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tokens = api_token_queries::list_api_tokens_by_user(&state.db, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tokens_json: Vec<_> = tokens
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "token": format!("{}...{}", &t.token[..10], &t.token[t.token.len()-4..]),
                "last_used_at": t.last_used_at,
                "created_at": t.created_at,
            })
        })
        .collect();

    Ok(Json(json!({ "tokens": tokens_json })))
}

pub async fn delete_token(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(token_id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let deleted = api_token_queries::delete_api_token(&state.db, token_id, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !deleted {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(response::ok().1)
}

pub async fn update_user_api_access(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<i64>,
    Json(payload): Json<crate::db::models::UpdateUserApiAccessRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let updated = api_token_queries::update_user_api_access(&state.db, user_id, payload.api_enabled)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !updated {
        return Err(StatusCode::NOT_FOUND);
    }

    if !payload.api_enabled {
        let deleted_count = api_token_queries::delete_all_user_api_tokens(&state.db, user_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        tracing::info!("Disabled API access for user {}, deleted {} tokens", user_id, deleted_count);
    }

    Ok(Json(json!({
        "ok": true,
        "api_enabled": payload.api_enabled,
    })))
}
