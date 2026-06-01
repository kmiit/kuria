use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Extension, Path, State},
};
use serde_json::json;

use crate::db::models::CreateDomainRequest;
use crate::db::queries;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

pub async fn list_domains(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let domains = queries::list_domains(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "domains": domains })))
}

pub async fn create_domain(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateDomainRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let domain = queries::create_domain(&state.db, &payload.domain_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "domain": domain })))
}

pub async fn delete_domain(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    queries::delete_domain(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn generate_dkim(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(_id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // In a real implementation, generate RSA key pair here
    // For now, return a placeholder
    let selector = &state.config.dkim.selector;

    Ok(Json(json!({
        "ok": true,
        "message": "DKIM key generation not yet implemented. Use openssl to generate keys manually.",
        "dns_record": format!("{}._domainkey IN TXT \"v=DKIM1; k=rsa; p=YOUR_PUBLIC_KEY\"", selector),
    })))
}
