use axum::body::{Body, to_bytes};
use axum::extract::DefaultBodyLimit;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, delete, get, post, put};
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use super::handlers::*;
use super::middleware::auth_middleware;
use super::rate_limit::LoginRateLimiter;
use crate::config::Config;
use crate::plugin::PluginManager;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sqlx::SqlitePool,
    pub plugins: Arc<PluginManager>,
    pub login_rate_limiter: Arc<LoginRateLimiter>,
}

async fn spa_index() -> Html<String> {
    // Serve the Vue SPA index.html
    let html = match std::fs::read_to_string("static/dist/index.html") {
        Ok(content) => content,
        Err(_) => {
            // Fallback if frontend not built
            r#"<!DOCTYPE html>
<html><head><title>Kuria Mail</title></head>
<body>
<h1>📧 Kuria Mail Server</h1>
<p>Frontend not built. Run: <code>cd frontend && npm run build</code></p>
<p>API is available at <a href="/api/health">/api/health</a></p>
</body></html>"#
                .to_string()
        }
    };
    Html(html)
}

async fn api_not_found() -> (StatusCode, Json<serde_json::Value>) {
    json_error_response(StatusCode::NOT_FOUND, "API route not found")
}

pub fn create_router(
    config: Arc<Config>,
    db: sqlx::SqlitePool,
    plugins: Arc<PluginManager>,
) -> Router {
    let state = AppState {
        config,
        db,
        plugins,
        login_rate_limiter: Arc::new(LoginRateLimiter::new()),
    };

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/api/auth/login", post(auth::login))
        .route("/api/health", get(|| async { "OK" }))
        .route("/api/setup/status", get(settings::check_setup))
        .route("/api/setup", post(settings::run_setup))
        .route(
            "/api/plugins/{plugin}/webhook/{*path}",
            any(settings::plugin_webhook),
        )
        .route(
            "/plugin-assets/{plugin}/{*path}",
            get(settings::plugin_asset),
        )
        .route("/api/{*path}", any(api_not_found));

    // Protected routes (auth required)
    let protected_routes = Router::new()
        // Emails
        .route("/api/emails", get(mailbox::list_emails))
        .route("/api/emails/{id}", get(mailbox::get_email))
        .route("/api/emails/{id}", delete(mailbox::delete_email))
        .route("/api/emails/{id}/read", put(mailbox::mark_read))
        .route("/api/emails/{id}/unread", put(mailbox::mark_unread))
        .route("/api/emails/{id}/move", put(mailbox::move_email))
        .route("/api/emails/send", post(mailbox::send_email))
        .route("/api/emails/mailboxes", get(mailbox::get_mailbox_counts))
        .route("/api/trash", delete(mailbox::empty_trash))
        .route("/api/drafts", post(mailbox::save_draft))
        .route("/api/drafts/{id}", get(mailbox::get_draft))
        .route("/api/drafts/{id}", delete(mailbox::delete_draft))
        .route("/api/attachments/{id}", get(mailbox::download_attachment))
        // Domains
        .route("/api/domains", get(domain::list_domains))
        .route("/api/domains", post(domain::create_domain))
        .route("/api/domains/{id}", delete(domain::delete_domain))
        .route("/api/domains/{id}/dkim", post(domain::generate_dkim))
        // Users
        .route("/api/users", get(user::list_users))
        .route("/api/users", post(user::create_user))
        .route("/api/users/{id}", delete(user::delete_user))
        // Settings
        .route("/api/settings", get(settings::get_settings))
        .route("/api/settings", put(settings::update_settings))
        .route("/api/settings/password", post(settings::change_password))
        .route("/api/plugins", get(settings::get_plugins))
        .route(
            "/api/plugins/{plugin}/api/{*path}",
            any(settings::plugin_api),
        )
        // Outbound queue
        .route("/api/queue", get(queue::list_queue))
        .route("/api/queue/{id}/retry", post(queue::retry_queue_item))
        .route("/api/queue/{id}", delete(queue::delete_queue_item))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Static files with SPA fallback
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .nest_service("/assets", ServeDir::new("static/dist/assets"))
        .fallback(spa_index)
        .layer(axum::middleware::from_fn(api_error_middleware))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            plugin_middleware,
        ))
        .layer(DefaultBodyLimit::max(40 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn error_message_for_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "Bad request",
        StatusCode::UNAUTHORIZED => "Unauthorized",
        StatusCode::FORBIDDEN => "Forbidden",
        StatusCode::NOT_FOUND => "Not found",
        StatusCode::CONFLICT => "Conflict",
        StatusCode::PAYLOAD_TOO_LARGE => "Payload too large",
        StatusCode::TOO_MANY_REQUESTS => "Too many failed login attempts",
        StatusCode::UNPROCESSABLE_ENTITY => "Invalid request body",
        StatusCode::INTERNAL_SERVER_ERROR => "Internal server error",
        _ => status.canonical_reason().unwrap_or("Request failed"),
    }
}

fn json_error_response(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({
            "error": message.into(),
            "status": status.as_u16(),
        })),
    )
}

async fn api_error_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let is_api = request.uri().path().starts_with("/api/");
    let response = next.run(request).await;

    if !is_api || !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }

    let status = response.status();
    let headers = response.headers().clone();
    let (mut parts, body) = response.into_parts();
    let body_bytes = match to_bytes(body, 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return json_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read response",
            )
            .into_response();
        }
    };

    if !body_bytes.is_empty() {
        parts.headers = headers;
        return Response::from_parts(parts, Body::from(body_bytes));
    }

    let mut response =
        json_error_response(status, error_message_for_status(status)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

/// Plugin middleware: calls on_web_request for every HTTP request.
async fn plugin_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or("").to_string();

    // Collect headers into a JSON object
    let mut headers_map: serde_json::Map<String, serde_json::Value> = request
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.to_string(), serde_json::Value::String(v.to_string())))
        })
        .collect();
    if state.config.web.trust_proxy_headers {
        add_trusted_proxy_headers(request.headers(), &mut headers_map);
    }
    let headers_json = serde_json::to_string(&headers_map).unwrap_or_default();

    if let Some(result) = state
        .plugins
        .call_web_request(&method, &path, &headers_json, &query)
        && result.reject
    {
        let msg = result
            .reject_message
            .unwrap_or_else(|| "Request rejected by plugin".to_string());
        tracing::warn!("Plugin rejected request: {} {} - {}", method, path, msg);
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

fn first_header_value<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn add_proxy_header(
    headers_map: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        headers_map.insert(
            name.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

fn add_trusted_proxy_headers(
    headers: &axum::http::HeaderMap,
    headers_map: &mut serde_json::Map<String, serde_json::Value>,
) {
    add_proxy_header(
        headers_map,
        "x-kuria-client-ip",
        first_header_value(headers, "x-forwarded-for")
            .or_else(|| first_header_value(headers, "x-real-ip")),
    );
    add_proxy_header(
        headers_map,
        "x-kuria-forwarded-proto",
        first_header_value(headers, "x-forwarded-proto"),
    );
    add_proxy_header(
        headers_map,
        "x-kuria-forwarded-host",
        first_header_value(headers, "x-forwarded-host")
            .or_else(|| first_header_value(headers, "host")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_errors_have_stable_messages() {
        assert_eq!(
            error_message_for_status(StatusCode::BAD_REQUEST),
            "Bad request"
        );
        assert_eq!(
            error_message_for_status(StatusCode::UNAUTHORIZED),
            "Unauthorized"
        );
        assert_eq!(
            error_message_for_status(StatusCode::INTERNAL_SERVER_ERROR),
            "Internal server error"
        );
    }

    #[test]
    fn json_error_response_contains_error_and_status() {
        let (status, Json(body)) = json_error_response(StatusCode::CONFLICT, "Domain has users");
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "Domain has users");
        assert_eq!(body["status"], 409);
    }
}
