use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{any, delete, get, post, put};
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use super::handlers::*;
use super::middleware::auth_middleware;
use crate::config::Config;
use crate::plugin::PluginManager;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sqlx::SqlitePool,
    pub plugins: Arc<PluginManager>,
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
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "API route not found",
        })),
    )
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
    };

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/api/auth/login", post(auth::login))
        .route("/api/health", get(|| async { "OK" }))
        .route("/api/setup/status", get(settings::check_setup))
        .route("/api/setup", post(settings::run_setup))
        .route("/api/{*path}", any(api_not_found));

    // Protected routes (auth required)
    let protected_routes = Router::new()
        // Emails
        .route("/api/emails", get(mailbox::list_emails))
        .route("/api/emails/{id}", get(mailbox::get_email))
        .route("/api/emails/{id}", delete(mailbox::delete_email))
        .route("/api/emails/{id}/read", put(mailbox::mark_read))
        .route("/api/emails/{id}/move", put(mailbox::move_email))
        .route("/api/emails/send", post(mailbox::send_email))
        .route("/api/emails/mailboxes", get(mailbox::get_mailbox_counts))
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
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            plugin_middleware,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
    {
        if result.reject {
            let msg = result
                .reject_message
                .unwrap_or_else(|| "Request rejected by plugin".to_string());
            tracing::warn!("Plugin rejected request: {} {} - {}", method, path, msg);
            return Err(StatusCode::FORBIDDEN);
        }
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
