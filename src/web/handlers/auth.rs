use axum::http::StatusCode;
use axum::{Json, extract::State};
use bcrypt::verify;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;

use crate::db::models::LoginRequest;
use crate::db::queries;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

fn normalize_login_email(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub fn jwt_expiration_24h() -> Result<usize, StatusCode> {
    chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(24))
        .map(|expires| expires.timestamp() as usize)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let email = normalize_login_email(&payload.email);
    if state.login_rate_limiter.is_limited(&email) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    let Some(user) = queries::get_user_by_email(&state.db, &email)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        state.login_rate_limiter.record_failure(&email);
        return Err(StatusCode::UNAUTHORIZED);
    };

    let valid = verify(&payload.password, &user.password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !valid {
        state.login_rate_limiter.record_failure(&email);
        return Err(StatusCode::UNAUTHORIZED);
    }
    state.login_rate_limiter.record_success(&email);

    let claims = Claims {
        sub: user.id,
        email: user.email.clone(),
        is_admin: user.is_admin,
        exp: jwt_expiration_24h()?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_email_is_normalized() {
        assert_eq!(
            normalize_login_email(" User@Example.COM "),
            "user@example.com"
        );
    }

    #[test]
    fn jwt_expiration_is_in_the_future() {
        assert!(
            jwt_expiration_24h().expect("expiration") > chrono::Utc::now().timestamp() as usize
        );
    }
}
