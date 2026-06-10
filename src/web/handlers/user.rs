use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Extension, Path, State},
};
use serde_json::json;

use crate::db::models::CreateUserRequest;
use crate::db::queries;
use crate::web::middleware::Claims;
use crate::web::{response, router::AppState};

fn normalize_email(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_valid_domain(value: &str) -> bool {
    let mut labels = value.split('.').peekable();
    if labels.peek().is_none() || !value.contains('.') || value.len() > 253 {
        return false;
    }

    labels.all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

fn is_valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };

    !local.is_empty()
        && local.len() <= 64
        && !local.contains(char::is_whitespace)
        && is_valid_domain(domain)
}

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
                "api_enabled": u.api_enabled,
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

    let email = normalize_email(&payload.email);
    if !is_valid_email(&email) || payload.password.len() < 6 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let domain = queries::get_domain_by_id(&state.db, payload.domain_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::BAD_REQUEST)?;

    let email_domain = email.split('@').next_back().unwrap_or_default();
    if email_domain != domain.domain_name {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Hash password
    let password_hash =
        bcrypt::hash(&payload.password, 10).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = queries::create_user(
        &state.db,
        &email,
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
            "api_enabled": user.api_enabled,
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

    let deleted = queries::delete_user(&state.db, id)
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
    fn email_validation_rejects_bad_addresses() {
        assert_eq!(normalize_email(" User@Example.COM "), "user@example.com");
        assert!(is_valid_email("user@example.com"));
        assert!(!is_valid_email("user@localhost"));
        assert!(!is_valid_email("bad address@example.com"));
    }
}
