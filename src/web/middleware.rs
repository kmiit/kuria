use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

use super::router::AppState;
use crate::db::{api_token_queries, queries};

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct Claims {
    pub sub: i64, // user_id
    pub email: String,
    pub is_admin: bool,
    pub exp: usize,
}

fn bearer_token(header: Option<&str>) -> Option<&str> {
    header.and_then(|header| header.strip_prefix("Bearer "))
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());
    let token = bearer_token(auth_header).ok_or(StatusCode::UNAUTHORIZED)?;

    // Try API token first
    if token.starts_with("krt_") {
        let api_token = api_token_queries::get_api_token_by_token(&state.db, token)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let user = queries::get_user_by_id(&state.db, api_token.user_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if !user.api_enabled {
            return Err(StatusCode::FORBIDDEN);
        }

        // Update last used timestamp (fire and forget)
        tokio::spawn({
            let db = state.db.clone();
            let token = token.to_string();
            async move {
                let _ = api_token_queries::update_api_token_last_used(&db, &token).await;
            }
        });

        let claims = Claims {
            sub: user.id,
            email: user.email,
            is_admin: user.is_admin,
            exp: usize::MAX, // API tokens don't expire
        };

        let mut request = request;
        request.extensions_mut().insert(claims);
        return Ok(next.run(request).await);
    }

    // Fall back to JWT
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_requires_authorization_scheme() {
        assert_eq!(bearer_token(Some("Bearer abc.def")), Some("abc.def"));
        assert_eq!(bearer_token(Some("token=abc.def")), None);
        assert_eq!(bearer_token(None), None);
    }
}
