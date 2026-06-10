use axum::body::{Body, to_bytes};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use tokio::net::UdpSocket;

use crate::config::{HOSTNAME_SETTING_KEY, effective_hostname};
use crate::db::models::ChangePasswordRequest;
use crate::db::queries;
use crate::web::middleware::Claims;
use crate::web::router::AppState;

async fn configured_hostname(state: &AppState) -> anyhow::Result<String> {
    Ok(effective_hostname(&state.config, &state.db).await)
}

const PUBLIC_IPV4_SETTING_KEY: &str = "server.public_ipv4";
const PUBLIC_IPV6_SETTING_KEY: &str = "server.public_ipv6";

async fn detect_route_ip(bind_addr: &str, remote_addr: &str) -> Option<IpAddr> {
    let socket = UdpSocket::bind(bind_addr).await.ok()?;
    socket.connect(remote_addr).await.ok()?;
    Some(socket.local_addr().ok()?.ip())
}

async fn detected_server_ips() -> serde_json::Value {
    let (ipv4, ipv6) = tokio::join!(
        detect_route_ip("0.0.0.0:0", "1.1.1.1:80"),
        detect_route_ip("[::]:0", "[2606:4700:4700::1111]:80"),
    );

    json!({
        "ipv4": ip_detection_result(ipv4),
        "ipv6": ip_detection_result(ipv6),
    })
}

fn ip_detection_result(ip: Option<IpAddr>) -> serde_json::Value {
    match ip {
        Some(ip) => json!({
            "address": ip.to_string(),
            "public": is_public_ip(ip),
        }),
        None => serde_json::Value::Null,
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    let is_carrier_nat = octets[0] == 100 && (64..=127).contains(&octets[1]);
    let is_protocol_assignment = octets[0] == 192 && octets[1] == 0 && octets[2] == 0;
    let is_benchmark = octets[0] == 198 && (18..=19).contains(&octets[1]);
    let is_reserved = octets[0] == 0 || octets[0] >= 224;

    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || is_carrier_nat
        || is_protocol_assignment
        || is_benchmark
        || is_reserved)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    let first = segments[0];
    let is_global_unicast = (first & 0xe000) == 0x2000;
    let is_unique_local = (first & 0xfe00) == 0xfc00;
    let is_link_local = (first & 0xffc0) == 0xfe80;
    let is_documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;

    is_global_unicast
        && !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !is_unique_local
        && !is_link_local
        && !is_documentation
}

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

    let hostname = configured_hostname(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "initialized": initialized,
        "user_count": users.len(),
        "domain_count": domains.len(),
        "hostname": hostname,
    })))
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub hostname: String,
    pub domain: String,
    pub admin_email: String,
    pub admin_password: String,
    pub public_ipv4: Option<String>,
    pub public_ipv6: Option<String>,
    pub use_nginx_proxy: Option<bool>,
    pub smtp_port: Option<u16>,
    pub imap_port: Option<u16>,
    pub pop3_port: Option<u16>,
    pub web_port: Option<u16>,
    pub jwt_secret: Option<String>,
}

fn normalize_domain(value: &str) -> String {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    let value = if lower.starts_with("http://") {
        &value[7..]
    } else if lower.starts_with("https://") {
        &value[8..]
    } else {
        value
    };

    value
        .trim()
        .split('/')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

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

fn normalize_ip(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some(String::new());
    }

    value.parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

async fn manual_public_ips(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let ipv4 = queries::get_system_setting(&state.db, PUBLIC_IPV4_SETTING_KEY).await?;
    let ipv6 = queries::get_system_setting(&state.db, PUBLIC_IPV6_SETTING_KEY).await?;

    Ok(json!({
        "ipv4": ipv4.unwrap_or_default(),
        "ipv6": ipv6.unwrap_or_default(),
    }))
}

async fn save_public_ip_setting(
    state: &AppState,
    key: &str,
    value: &str,
    expected_version: fn(IpAddr) -> bool,
) -> Result<(), StatusCode> {
    let normalized = normalize_ip(value).ok_or(StatusCode::BAD_REQUEST)?;
    if !normalized.is_empty() {
        let ip = normalized
            .parse::<IpAddr>()
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if !expected_version(ip) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    queries::set_system_setting(&state.db, key, &normalized)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn is_ipv4(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(_))
}

fn is_ipv6(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V6(_))
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

    let hostname = normalize_domain(&payload.hostname);
    let domain_name = normalize_domain(&payload.domain);
    let admin_email = normalize_email(&payload.admin_email);

    if !is_valid_domain(&hostname)
        || !is_valid_domain(&domain_name)
        || !is_valid_email(&admin_email)
        || !admin_email.ends_with(&format!("@{}", domain_name))
        || payload.admin_password.len() < 6
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Create domain
    let domain = queries::create_domain(&state.db, &domain_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Generate DKIM key automatically
    let selector = state.config.dkim.selector.trim();
    let key_bits = state.config.dkim.key_size.max(2048) as usize;
    let (domain, _dkim_selector, dkim_dns_record) =
        crate::web::handlers::domain::generate_dkim_for_domain(
            &state.db,
            domain.id,
            &domain_name,
            selector,
            key_bits,
            None,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    queries::set_system_setting(&state.db, HOSTNAME_SETTING_KEY, &hostname)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(ref public_ipv4) = payload.public_ipv4 {
        save_public_ip_setting(&state, PUBLIC_IPV4_SETTING_KEY, public_ipv4, is_ipv4).await?;
    }

    if let Some(ref public_ipv6) = payload.public_ipv6 {
        save_public_ip_setting(&state, PUBLIC_IPV6_SETTING_KEY, public_ipv6, is_ipv6).await?;
    }

    // Create admin user
    let password_hash =
        bcrypt::hash(&payload.admin_password, 10).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = queries::create_user(&state.db, &admin_email, &password_hash, domain.id, true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!(
        "Setup completed: admin user {} created for domain {}",
        user.email,
        domain_name
    );

    // Generate nginx config if requested
    let mut nginx_config = None;
    if payload.use_nginx_proxy.unwrap_or(false)
        && let Ok(conf) = generate_nginx_config(&hostname, &state.config).await
    {
        nginx_config = Some(conf);
    }

    // Save config.toml with updated settings
    save_config_file(&payload, &state.config).await.ok();

    // Reload config to get the new JWT secret
    let new_config = crate::config::Config::load("config.toml")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Start mail services dynamically
    start_mail_services(&state).await;

    // Generate JWT token for immediate login
    let claims = crate::web::middleware::Claims {
        sub: user.id,
        email: user.email.clone(),
        is_admin: user.is_admin,
        exp: crate::web::handlers::auth::jwt_expiration_24h()?,
    };

    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    header.typ = Some("JWT".to_string());

    let token = jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(new_config.web.jwt_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "ok": true,
        "hostname": hostname,
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
        "detected_ips": detected_server_ips().await,
        "manual_public_ips": manual_public_ips(&state).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        "dns_records": {
            "mx": format!("{}  IN  MX  10  {}", domain_name, hostname),
            "spf": format!("{}  IN  TXT  \"v=spf1 mx:{} -all\"", domain_name, domain_name),
            "dkim": dkim_dns_record,
            "dmarc": format!("_dmarc.{}  IN  TXT  \"v=DMARC1; p=quarantine; rua=mailto:admin@{}\"", domain_name, domain_name),
            "bimi": format!("default._bimi.{}  IN  TXT  \"v=BIMI1;\"", domain_name),
        },
        "nginx_config": nginx_config,
    })))
}

pub async fn get_settings(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // Load current config from file to show latest values
    let current_config = crate::config::Config::load("config.toml")
        .unwrap_or_else(|_| (*state.config).clone());

    let plugins = plugin_status(&state);
    let hostname = configured_hostname(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "hostname": hostname,
        "smtp_port": listen_port(&current_config.smtp.listen_addr),
        "imap_port": listen_port(&current_config.imap.listen_addr),
        "pop3_port": listen_port(&current_config.pop3.listen_addr),
        "web_port": listen_port(&current_config.web.listen_addr),
        "dkim_selector": current_config.dkim.selector,
        "detected_ips": detected_server_ips().await,
        "manual_public_ips": manual_public_ips(&state).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        "plugins": plugins,
        "smtp_running": *state.mail_services.smtp_running.read().await,
        "imap_running": *state.mail_services.imap_running.read().await,
        "pop3_running": *state.mail_services.pop3_running.read().await,
    })))
}

pub async fn get_plugins(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(Json(plugin_status(&state)))
}

pub async fn plugin_api(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((plugin, path)): Path<(String, String)>,
    request: Request,
) -> Result<Response, StatusCode> {
    let method = request.method().to_string();
    let query = request.uri().query().unwrap_or("").to_string();
    let headers_json = request_headers_json(request.headers());
    let user_json =
        serde_json::to_string(&claims).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path.trim_start_matches('/'))
    };
    let body = to_bytes(request.into_body(), 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let body_json = if body.is_empty() {
        "{}".to_string()
    } else {
        String::from_utf8(body.to_vec()).map_err(|_| StatusCode::BAD_REQUEST)?
    };

    let response = state
        .plugins
        .call_plugin_api(
            &plugin,
            &method,
            &path,
            &headers_json,
            &query,
            &body_json,
            &user_json,
        )
        .ok_or(StatusCode::NOT_FOUND)?;
    let status = StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::OK);
    let mut response = Response::new(Body::from(response.body_json));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    Ok(response)
}

pub async fn plugin_asset(
    State(state): State<AppState>,
    Path((plugin, path)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    let plugin_response = state
        .plugins
        .call_plugin_admin_asset(&plugin, &path)
        .ok_or(StatusCode::NOT_FOUND)?;
    let status = StatusCode::from_u16(plugin_response.status_code).unwrap_or(StatusCode::OK);
    let content_type = plugin_asset_content_type(&path);
    let cache_control = if content_type == "text/html; charset=utf-8" {
        "no-store"
    } else {
        "public, max-age=300"
    };

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(plugin_response.body_json))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn plugin_webhook(
    State(state): State<AppState>,
    Path((plugin, path)): Path<(String, String)>,
    request: Request,
) -> Result<Response, StatusCode> {
    let method = request.method().to_string();
    let query = request.uri().query().unwrap_or("").to_string();
    let headers_json = request_headers_json(request.headers());
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path.trim_start_matches('/'))
    };
    let body = to_bytes(request.into_body(), 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let body_json = if body.is_empty() {
        "{}".to_string()
    } else {
        String::from_utf8(body.to_vec()).map_err(|_| StatusCode::BAD_REQUEST)?
    };

    let plugin_response = state
        .plugins
        .call_plugin_webhook(&plugin, &method, &path, &headers_json, &query, &body_json)
        .ok_or(StatusCode::NOT_FOUND)?;
    let status = StatusCode::from_u16(plugin_response.status_code).unwrap_or(StatusCode::OK);
    let body_json = plugin_response.body_json;

    if status.is_success() {
        send_plugin_outbound_emails(&state, &body_json).await;
    }

    let mut response = Response::new(Body::from(body_json));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    Ok(response)
}

async fn send_plugin_outbound_emails(state: &AppState, body_json: &str) {
    let Ok(response) =
        serde_json::from_str::<crate::plugin::PluginOutboundEmailResponse>(body_json)
    else {
        tracing::warn!("Plugin webhook returned non-standard JSON response");
        return;
    };

    if response.outbound_emails.is_empty() {
        return;
    }

    let delivery = crate::mail::delivery::MailDelivery::with_plugins(
        state.config.clone(),
        state.db.clone(),
        state.plugins.clone(),
    );
    for email in response.outbound_emails {
        if email.from.trim().is_empty() || email.to.is_empty() || email.subject.trim().is_empty() {
            tracing::warn!("Plugin webhook returned an invalid outbound email");
            continue;
        }

        if let Err(error) = delivery
            .send_composed_email(crate::mail::delivery::ComposedEmail {
                from: &email.from,
                to: &email.to,
                cc: &email.cc,
                bcc: &email.bcc,
                subject: &email.subject,
                body_text: email.body_text.as_deref(),
                body_html: email.body_html.as_deref(),
                attachments: &[],
            })
            .await
        {
            tracing::warn!("Failed to send plugin outbound email: {}", error);
        }
    }
}

fn request_headers_json(headers: &HeaderMap) -> String {
    let headers_map: serde_json::Map<String, serde_json::Value> = headers
        .iter()
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|value| {
                (
                    name.to_string(),
                    serde_json::Value::String(value.to_string()),
                )
            })
        })
        .collect();
    serde_json::to_string(&headers_map).unwrap_or_default()
}

fn plugin_asset_content_type(path: &str) -> &'static str {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        _ => "text/html; charset=utf-8",
    }
}

fn plugin_status(state: &AppState) -> serde_json::Value {
    let plugins_config = state.config.plugins.as_ref();
    let enabled = plugins_config.map(|pc| pc.enabled).unwrap_or(false);
    let configured_paths = plugins_config
        .map(|pc| pc.paths.clone())
        .unwrap_or_default();
    let directory = plugins_config.and_then(|pc| pc.directory.clone());
    let loaded = state.plugins.plugins_info();
    let load_errors = state.plugins.load_errors();
    let loaded_count = loaded.len();
    let configured_count = configured_paths.len();

    json!({
        "enabled": enabled,
        "directory": directory,
        "configured_paths": configured_paths,
        "loaded": loaded,
        "load_errors": load_errors,
        "loaded_count": loaded_count,
        "configured_count": configured_count,
        "abi_version": kuria_plugin::PLUGIN_ABI_VERSION,
    })
}

fn listen_port(addr: &str) -> Option<u16> {
    if let Ok(socket_addr) = addr.parse::<std::net::SocketAddr>() {
        return Some(socket_addr.port());
    }

    addr.rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub hostname: Option<String>,
    pub public_ipv4: Option<String>,
    pub public_ipv6: Option<String>,
}

pub async fn update_settings(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<UpdateSettingsRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    if let Some(hostname) = payload.hostname {
        let hostname = normalize_domain(&hostname);
        if !is_valid_domain(&hostname) {
            return Err(StatusCode::BAD_REQUEST);
        }

        queries::set_system_setting(&state.db, HOSTNAME_SETTING_KEY, &hostname)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(public_ipv4) = payload.public_ipv4 {
        save_public_ip_setting(&state, PUBLIC_IPV4_SETTING_KEY, &public_ipv4, is_ipv4).await?;
    }

    if let Some(public_ipv6) = payload.public_ipv6 {
        save_public_ip_setting(&state, PUBLIC_IPV6_SETTING_KEY, &public_ipv6, is_ipv6).await?;
    }

    let hostname = configured_hostname(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "ok": true,
        "message": "Settings updated",
        "hostname": hostname,
        "manual_public_ips": manual_public_ips(&state).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    })))
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if payload.new_password.len() < 6 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let user = queries::get_user_by_id(&state.db, claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Verify old password
    let valid = bcrypt::verify(&payload.old_password, &user.password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !valid {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Hash and save new password
    let new_hash =
        bcrypt::hash(&payload.new_password, 10).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    queries::update_user_password(&state.db, claims.sub, &new_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "ok": true, "message": "密码已修改" })))
}

async fn generate_nginx_config(hostname: &str, config: &crate::config::Config) -> anyhow::Result<String> {
    let smtp_port = listen_port(&config.smtp.listen_addr).unwrap_or(25);
    let imap_port = listen_port(&config.imap.listen_addr).unwrap_or(143);
    let pop3_port = listen_port(&config.pop3.listen_addr).unwrap_or(110);
    let web_port = listen_port(&config.web.listen_addr).unwrap_or(8080);

    let nginx_conf = format!(r#"# Kuria Mail Server - Nginx Configuration
# Generated by Kuria Setup Wizard
#
# 使用方法：
# 1. 将本文件保存为 /etc/nginx/kuria-mail.conf
# 2. 在 nginx.conf 的 stream 和 http 块中分别 include：
#    stream {{
#        include /etc/nginx/kuria-mail-stream.conf;
#    }}
#    http {{
#        include /etc/nginx/kuria-mail-http.conf;
#    }}
# 3. 修改证书路径
# 4. 执行 nginx -t 测试配置，然后 systemctl reload nginx

# ============ kuria-mail-stream.conf (放在 stream 块中) ============

# SMTPS (port 465) -> SMTP plain (port {smtp_port})
upstream kuria_smtp {{
    server 127.0.0.1:{smtp_port};
}}

server {{
    listen 465 ssl;
    proxy_pass kuria_smtp;
    proxy_connect_timeout 1s;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;
}}

# IMAPS (port 993) -> IMAP plain (port {imap_port})
upstream kuria_imap {{
    server 127.0.0.1:{imap_port};
}}

server {{
    listen 993 ssl;
    proxy_pass kuria_imap;
    proxy_connect_timeout 1s;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;
}}

# POP3S (port 995) -> POP3 plain (port {pop3_port})
upstream kuria_pop3 {{
    server 127.0.0.1:{pop3_port};
}}

server {{
    listen 995 ssl;
    proxy_pass kuria_pop3;
    proxy_connect_timeout 1s;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;
}}

# ============ kuria-mail-http.conf (放在 http 块中) ============

# Web UI HTTPS (port 443) -> HTTP (port {web_port})
upstream kuria_web {{
    server 127.0.0.1:{web_port};
}}

server {{
    listen 443 ssl http2;
    server_name {hostname};

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;
    ssl_prefer_server_ciphers on;

    location / {{
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }}
}}
"#);

    std::fs::write("nginx-kuria.conf", &nginx_conf)?;
    Ok(nginx_conf)
}

async fn save_config_file(payload: &SetupRequest, base_config: &crate::config::Config) -> anyhow::Result<()> {
    use crate::config::*;

    let mut config = base_config.clone();
    config.server.hostname = payload.hostname.clone();

    if let Some(port) = payload.smtp_port {
        config.smtp.listen_addr = format!("0.0.0.0:{}", port);
    }
    if let Some(port) = payload.imap_port {
        config.imap.listen_addr = format!("0.0.0.0:{}", port);
    }
    if let Some(port) = payload.pop3_port {
        config.pop3.listen_addr = format!("0.0.0.0:{}", port);
    }
    if let Some(port) = payload.web_port {
        config.web.listen_addr = format!("0.0.0.0:{}", port);
    }
    if let Some(secret) = &payload.jwt_secret {
        if !secret.is_empty() {
            config.web.jwt_secret = secret.clone();
        }
    } else {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        config.web.jwt_secret = format!("kuria-{:x}-{}", timestamp, std::process::id());
    }

    if payload.use_nginx_proxy.unwrap_or(false) {
        config.tls.mode = TlsMode::External;
        config.smtp.listen_addr_tls = "0.0.0.0:0".to_string();
        config.imap.listen_addr_tls = "0.0.0.0:0".to_string();
        config.pop3.listen_addr_tls = "0.0.0.0:0".to_string();
    }

    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write("config.toml", toml_str)?;
    Ok(())
}

async fn start_mail_services(state: &AppState) {
    if state.mail_services.is_running().await {
        return;
    }

    let config = match crate::config::Config::load("config.toml") {
        Ok(c) => std::sync::Arc::new(c),
        Err(e) => {
            tracing::error!("Failed to load config: {}", e);
            return;
        }
    };

    let internal_tls = crate::tls::config::internal_tls_status(&config.tls);

    // Start SMTP
    let smtp_listener = match tokio::net::TcpListener::bind(&config.smtp.listen_addr).await {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::error!("Failed to bind SMTP port {}: {}", config.smtp.listen_addr, e);
            None
        }
    };

    if let Some(smtp_listener) = smtp_listener {
        let smtps_listener = if !crate::mail_services::listener_disabled(&config.smtp.listen_addr_tls) && internal_tls.is_enabled() {
            match tokio::net::TcpListener::bind(&config.smtp.listen_addr_tls).await {
                Ok(l) => Some(l),
                Err(e) => {
                    tracing::warn!("Failed to bind SMTPS port {}: {}", config.smtp.listen_addr_tls, e);
                    None
                }
            }
        } else {
            None
        };

        let smtp_config = config.clone();
        let smtp_db = state.db.clone();
        let smtp_plugins = state.plugins.clone();
        let smtp_running = state.mail_services.smtp_running.clone();
        tokio::spawn(async move {
            *smtp_running.write().await = true;
            let server = crate::smtp::server::SmtpServer::new(smtp_config, smtp_db, smtp_plugins);
            if let Err(e) = server.start_with_listeners(smtp_listener, smtps_listener).await {
                tracing::error!("SMTP server error: {}", e);
            }
            *smtp_running.write().await = false;
        });
    }

    // Start IMAP
    let imap_listener = match tokio::net::TcpListener::bind(&config.imap.listen_addr).await {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::error!("Failed to bind IMAP port {}: {}", config.imap.listen_addr, e);
            None
        }
    };

    if let Some(imap_listener) = imap_listener {
        let imaps_listener = if !crate::mail_services::listener_disabled(&config.imap.listen_addr_tls) && internal_tls.is_enabled() {
            match tokio::net::TcpListener::bind(&config.imap.listen_addr_tls).await {
                Ok(l) => Some(l),
                Err(e) => {
                    tracing::warn!("Failed to bind IMAPS port {}: {}", config.imap.listen_addr_tls, e);
                    None
                }
            }
        } else {
            None
        };

        let imap_config = config.clone();
        let imap_db = state.db.clone();
        let imap_running = state.mail_services.imap_running.clone();
        tokio::spawn(async move {
            *imap_running.write().await = true;
            let server = crate::imap::server::ImapServer::new(imap_config, imap_db);
            if let Err(e) = server.start_with_listeners(imap_listener, imaps_listener).await {
                tracing::error!("IMAP server error: {}", e);
            }
            *imap_running.write().await = false;
        });
    }

    // Start POP3
    let pop3_listener = match tokio::net::TcpListener::bind(&config.pop3.listen_addr).await {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::error!("Failed to bind POP3 port {}: {}", config.pop3.listen_addr, e);
            None
        }
    };

    if let Some(pop3_listener) = pop3_listener {
        let pop3s_listener = if !crate::mail_services::listener_disabled(&config.pop3.listen_addr_tls) && internal_tls.is_enabled() {
            match tokio::net::TcpListener::bind(&config.pop3.listen_addr_tls).await {
                Ok(l) => Some(l),
                Err(e) => {
                    tracing::warn!("Failed to bind POP3S port {}: {}", config.pop3.listen_addr_tls, e);
                    None
                }
            }
        } else {
            None
        };

        let pop3_config = config.clone();
        let pop3_db = state.db.clone();
        let pop3_running = state.mail_services.pop3_running.clone();
        tokio::spawn(async move {
            *pop3_running.write().await = true;
            let server = crate::pop3::server::Pop3Server::new(pop3_config, pop3_db);
            if let Err(e) = server.start_with_listeners(pop3_listener, pop3s_listener).await {
                tracing::error!("POP3 server error: {}", e);
            }
            *pop3_running.write().await = false;
        });
    }

    tracing::info!("Mail services started dynamically");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ipv4_detection_rejects_private_and_reserved_ranges() {
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1))));
        assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }

    #[test]
    fn public_ipv6_detection_rejects_private_and_documentation_ranges() {
        assert!(!is_public_ip(IpAddr::V6("fd00::1".parse().unwrap())));
        assert!(!is_public_ip(IpAddr::V6("fe80::1".parse().unwrap())));
        assert!(!is_public_ip(IpAddr::V6("2001:db8::1".parse().unwrap())));
        assert!(is_public_ip(IpAddr::V6(
            "2606:4700:4700::1111".parse().unwrap()
        )));
    }

    #[test]
    fn manual_public_ip_values_are_normalized() {
        assert_eq!(normalize_ip("  8.8.8.8  "), Some("8.8.8.8".to_string()));
        assert_eq!(normalize_ip("  "), Some(String::new()));
        assert_eq!(normalize_ip("bad-ip"), None);
        assert!(is_ipv4("8.8.8.8".parse().unwrap()));
        assert!(!is_ipv4("2606:4700:4700::1111".parse().unwrap()));
        assert!(is_ipv6("2606:4700:4700::1111".parse().unwrap()));
        assert!(!is_ipv6("8.8.8.8".parse().unwrap()));
    }
}
