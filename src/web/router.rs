use std::sync::Arc;
use axum::Router;
use axum::response::Html;
use axum::routing::{get, post, put, delete};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use super::handlers::*;
use super::middleware::auth_middleware;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sqlx::SqlitePool,
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
</body></html>"#.to_string()
        }
    };
    Html(html)
}

pub fn create_router(config: Arc<Config>, db: sqlx::SqlitePool) -> Router {
    let state = AppState {
        config,
        db,
    };

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/api/auth/login", post(auth::login))
        .route("/api/health", get(|| async { "OK" }))
        .route("/api/setup/status", get(settings::check_setup))
        .route("/api/setup", post(settings::run_setup));

    // Protected routes (auth required)
    let protected_routes = Router::new()
        // Emails
        .route("/api/emails", get(mailbox::list_emails))
        .route("/api/emails/{id}", get(mailbox::get_email))
        .route("/api/emails/{id}", delete(mailbox::delete_email))
        .route("/api/emails/{id}/read", put(mailbox::mark_read))
        .route("/api/emails/{id}/move", put(mailbox::move_email))
        .route("/api/emails/send", post(mailbox::send_email))
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
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));

    // Static files with SPA fallback
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .nest_service("/assets", ServeDir::new("static/dist/assets"))
        .fallback(spa_index)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
