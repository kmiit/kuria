use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Extension, Path, State},
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand_core::OsRng;
use rsa::RsaPrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey, LineEnding as Pkcs1LineEnding};
use serde_json::json;
use uuid::Uuid;

use crate::db::models::CreateDomainRequest;
use crate::db::queries;
use crate::mail::auth::generate_dkim_dns_record;
use crate::web::middleware::Claims;
use crate::web::{response, router::AppState};

struct DkimKeyPair {
    private_key_pem: String,
    public_key_dns: String,
}

fn generate_dkim_key_pair(key_bits: usize) -> anyhow::Result<DkimKeyPair> {
    let mut rng = OsRng;
    let private_key = RsaPrivateKey::new(&mut rng, key_bits)?;
    let public_key = private_key.to_public_key();

    let private_key_pem = private_key.to_pkcs1_pem(Pkcs1LineEnding::LF)?.to_string();
    let public_key_der = public_key.to_pkcs1_der()?;
    let public_key_dns = STANDARD.encode(public_key_der.as_bytes());

    Ok(DkimKeyPair {
        private_key_pem,
        public_key_dns,
    })
}

fn generate_distinct_dkim_key_pair(
    key_bits: usize,
    existing_public_key: Option<&str>,
) -> anyhow::Result<DkimKeyPair> {
    for _ in 0..3 {
        let key_pair = generate_dkim_key_pair(key_bits)?;
        if Some(key_pair.public_key_dns.as_str()) != existing_public_key {
            return Ok(key_pair);
        }
    }

    anyhow::bail!("generated DKIM public key matched the existing key")
}

fn build_dkim_selector(base_selector: &str, existing_public_key: Option<&str>) -> String {
    if existing_public_key.is_some() {
        let random_suffix: String = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let suffix = format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%d%H%M%S"),
            random_suffix
        );
        let max_base_len = 63usize.saturating_sub(suffix.len() + 1);
        let rotation_base: String = base_selector.chars().take(max_base_len.max(1)).collect();

        format!("{}-{}", rotation_base, suffix)
    } else {
        base_selector.to_string()
    }
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

    let domain_name = normalize_domain(&payload.domain_name);
    if !is_valid_domain(&domain_name) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let domain = queries::create_domain(&state.db, &domain_name)
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

    let domain = queries::get_domain_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let user_count = queries::count_users_by_domain(&state.db, domain.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if user_count > 0 {
        return Err(StatusCode::CONFLICT);
    }

    queries::delete_domain(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(response::ok().1)
}

pub async fn generate_dkim_for_domain(
    db: &sqlx::SqlitePool,
    domain_id: i64,
    domain_name: &str,
    selector: &str,
    key_bits: usize,
    existing_public_key: Option<&str>,
) -> anyhow::Result<(crate::db::models::Domain, String, String)> {
    let selector = build_dkim_selector(selector, existing_public_key);
    let key_pair = generate_distinct_dkim_key_pair(key_bits, existing_public_key)?;

    let updated_domain = queries::update_domain_dkim(
        db,
        domain_id,
        &selector,
        &key_pair.private_key_pem,
        &key_pair.public_key_dns,
    )
    .await?;

    let dns_record = generate_dkim_dns_record(&selector, domain_name, &key_pair.public_key_dns);

    Ok((updated_domain, selector, dns_record))
}

pub async fn generate_dkim(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let domain = queries::get_domain_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let selector = state.config.dkim.selector.trim();
    if selector.is_empty()
        || !selector
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_')
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let key_bits = state.config.dkim.key_size.max(2048) as usize;
    let (updated_domain, selector, dns_record) = generate_dkim_for_domain(
        &state.db,
        domain.id,
        &domain.domain_name,
        selector,
        key_bits,
        domain.dkim_public_key.as_deref(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "ok": true,
        "message": "DKIM key generated.",
        "domain": updated_domain,
        "selector": selector,
        "dns_record": dns_record,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dkim_key_generation_produces_different_public_keys() {
        let first = generate_dkim_key_pair(2048).expect("first key");
        let second = generate_dkim_key_pair(2048).expect("second key");

        assert_ne!(first.public_key_dns, second.public_key_dns);
        assert_ne!(first.private_key_pem, second.private_key_pem);
        assert!(first.private_key_pem.contains("BEGIN RSA PRIVATE KEY"));
    }

    #[test]
    fn rotated_dkim_key_does_not_reuse_existing_public_key() {
        let existing = generate_dkim_key_pair(2048).expect("existing key");
        let rotated = generate_distinct_dkim_key_pair(2048, Some(&existing.public_key_dns))
            .expect("rotated key");

        assert_ne!(existing.public_key_dns, rotated.public_key_dns);
        assert_ne!(existing.private_key_pem, rotated.private_key_pem);
    }

    #[test]
    fn dkim_selector_changes_when_rotating_existing_key() {
        let selector = build_dkim_selector("kuria", Some("old-key"));
        assert!(selector.starts_with("kuria"));
        assert!(selector.len() > "kuria".len());
        assert!(selector.len() <= 63);
    }

    #[test]
    fn dkim_selector_is_unique_for_rapid_rotations() {
        let first = build_dkim_selector("kuria", Some("old-key"));
        let second = build_dkim_selector("kuria", Some("old-key"));

        assert_ne!(first, second);
    }

    #[test]
    fn domain_names_are_normalized_and_validated() {
        assert_eq!(
            normalize_domain(" HTTPS://Mail.Example.COM/path "),
            "mail.example.com"
        );
        assert!(is_valid_domain("example.com"));
        assert!(!is_valid_domain("localhost"));
        assert!(!is_valid_domain("-example.com"));
    }
}
