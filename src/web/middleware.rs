use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

use super::router::AppState;

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct Claims {
    pub sub: i64, // user_id
    pub email: String,
    pub is_admin: bool,
    pub exp: usize,
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Try Authorization header first, then query param
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    let token = if let Some(header) = auth_header {
        header.strip_prefix("Bearer ")
    } else {
        // Fall back to query parameter (for attachment downloads)
        request.uri().query().and_then(|q| {
            q.split('&')
                .find(|p| p.starts_with("token="))
                .map(|p| &p[6..])
        })
    };

    let token = token.ok_or(StatusCode::UNAUTHORIZED)?;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.config.web.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let mut request = request;
    request.extensions_mut().insert(token_data.claims);

    Ok(next.run(request).await)
}
