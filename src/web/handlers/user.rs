use axum::{
    extract::{Path, State, Extension},
    Json,
};
use axum::http::StatusCode;
use serde_json::json;

use crate::db::queries;
use crate::db::models::CreateUserRequest;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

pub async fn list_users(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let users = queries::list_users(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Remove password hashes from response
    let safe_users: Vec<serde_json::Value> = users
        .iter()
        .map(|u| {
            json!({
                "id": u.id,
                "email": u.email,
                "domain_id": u.domain_id,
                "is_admin": u.is_admin,
                "created_at": u.created_at,
            })
        })
        .collect();

    Ok(Json(json!({ "users": safe_users })))
}

pub async fn create_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // Hash password
    let password_hash = bcrypt::hash(&payload.password, 10)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = queries::create_user(
        &state.db,
        &payload.email,
        &password_hash,
        payload.domain_id,
        payload.is_admin,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "user": {
            "id": user.id,
            "email": user.email,
            "domain_id": user.domain_id,
            "is_admin": user.is_admin,
        }
    })))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // Prevent deleting yourself
    if id == claims.sub {
        return Err(StatusCode::BAD_REQUEST);
    }

    queries::delete_user(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}
