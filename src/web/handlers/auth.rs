use axum::http::StatusCode;
use axum::{Json, extract::State};
use bcrypt::verify;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;

use crate::db::models::LoginRequest;
use crate::db::queries;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let user = queries::get_user_by_email(&state.db, &payload.email)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let valid = verify(&payload.password, &user.password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !valid {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let claims = Claims {
        sub: user.id,
        email: user.email.clone(),
        is_admin: user.is_admin,
        exp: chrono::Utc::now()
            .checked_add_signed(chrono::Duration::hours(24))
            .unwrap()
            .timestamp() as usize,
    };

    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".to_string());

    let token = encode(
        &header,
        &claims,
        &EncodingKey::from_secret(state.config.web.jwt_secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!("JWT encode error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(json!({
        "token": token,
        "user": {
            "id": user.id,
            "email": user.email,
            "is_admin": user.is_admin,
        }
    })))
}
