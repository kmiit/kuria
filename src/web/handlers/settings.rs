use axum::{extract::State, Extension, Json};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use crate::db::queries;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

/// Check if the system has been initialized (has at least one admin user)
pub async fn check_setup(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let users = queries::list_users(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let domains = queries::list_domains(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let initialized = !users.is_empty();

    Ok(Json(json!({
        "initialized": initialized,
        "user_count": users.len(),
        "domain_count": domains.len(),
        "hostname": state.config.server.hostname,
    })))
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub hostname: String,
    pub domain: String,
    pub admin_email: String,
    pub admin_password: String,
}

/// Run the initial setup wizard
pub async fn run_setup(
    State(state): State<AppState>,
    Json(payload): Json<SetupRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Check if already initialized
    let users = queries::list_users(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !users.is_empty() {
        return Err(StatusCode::CONFLICT);
    }

    // Validate inputs
    if payload.hostname.is_empty() || payload.domain.is_empty()
        || payload.admin_email.is_empty() || payload.admin_password.len() < 6 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Create domain
    let domain = queries::create_domain(&state.db, &payload.domain)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create admin user
    let password_hash = bcrypt::hash(&payload.admin_password, 10)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = queries::create_user(
        &state.db,
        &payload.admin_email,
        &password_hash,
        domain.id,
        true,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!("Setup completed: admin user {} created for domain {}", user.email, payload.domain);

    // Generate JWT token for immediate login
    let claims = crate::web::middleware::Claims {
        sub: user.id,
        email: user.email.clone(),
        is_admin: user.is_admin,
        exp: chrono::Utc::now()
            .checked_add_signed(chrono::Duration::hours(24))
            .unwrap()
            .timestamp() as usize,
    };

    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    header.typ = Some("JWT".to_string());

    let token = jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(state.config.web.jwt_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "ok": true,
        "token": token,
        "user": {
            "id": user.id,
            "email": user.email,
            "is_admin": user.is_admin,
        },
        "domain": {
            "id": domain.id,
            "domain_name": domain.domain_name,
        },
        "dns_records": {
            "mx": format!("{}  IN  MX  10  {}", payload.domain, payload.hostname),
            "spf": format!("{}  IN  TXT  \"v=spf1 mx:{} -all\"", payload.domain, payload.domain),
            "dkim": format!("kuria._domainkey.{}  IN  TXT  \"v=DKIM1; k=rsa; p=YOUR_PUBLIC_KEY\"", payload.domain),
            "dmarc": format!("_dmarc.{}  IN  TXT  \"v=DMARC1; p=quarantine; rua=mailto:admin@{}\"", payload.domain, payload.domain),
        }
    })))
}

pub async fn get_settings(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(Json(json!({
        "hostname": state.config.server.hostname,
        "smtp_port": 25,
        "imap_port": 143,
        "web_port": 8080,
        "dkim_selector": state.config.dkim.selector,
    })))
}

pub async fn update_settings(
    State(_state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // TODO: Implement settings persistence
    Ok(Json(json!({ "ok": true, "message": "Settings updated" })))
}
