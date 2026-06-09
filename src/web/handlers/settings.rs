use axum::http::StatusCode;
use axum::{Extension, Json, extract::State};
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
}

fn normalize_domain(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
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

    queries::set_system_setting(&state.db, HOSTNAME_SETTING_KEY, &hostname)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
        "dns_records": {
            "mx": format!("{}  IN  MX  10  {}", domain_name, hostname),
            "spf": format!("{}  IN  TXT  \"v=spf1 mx:{} -all\"", domain_name, domain_name),
            "dkim": format!("After setup, open Domains and generate DKIM for {}", domain_name),
            "dmarc": format!("_dmarc.{}  IN  TXT  \"v=DMARC1; p=quarantine; rua=mailto:admin@{}\"", domain_name, domain_name),
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

    let plugins = plugin_status(&state);
    let hostname = configured_hostname(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "hostname": hostname,
        "smtp_port": listen_port(&state.config.smtp.listen_addr),
        "imap_port": listen_port(&state.config.imap.listen_addr),
        "web_port": listen_port(&state.config.web.listen_addr),
        "dkim_selector": state.config.dkim.selector,
        "detected_ips": detected_server_ips().await,
        "plugins": plugins,
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

    let hostname = configured_hostname(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "ok": true,
        "message": "Settings updated",
        "hostname": hostname,
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
}
