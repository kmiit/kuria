use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const HOSTNAME_SETTING_KEY: &str = "server.hostname";
const DEFAULT_JWT_SECRET: &str = "change-me-in-production";
const MIN_JWT_SECRET_LEN: usize = 32;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    pub smtp: SmtpConfig,
    pub imap: ImapConfig,
    pub pop3: Pop3Config,
    pub web: WebConfig,
    pub database: DatabaseConfig,
    pub tls: TlsConfig,
    pub dkim: DkimConfig,
    pub plugins: Option<PluginsConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub hostname: String,
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SmtpConfig {
    pub listen_addr: String,
    pub listen_addr_tls: String,
    pub enable_starttls: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImapConfig {
    pub listen_addr: String,
    pub listen_addr_tls: String,
    pub enable_starttls: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pop3Config {
    pub listen_addr: String,
    pub listen_addr_tls: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebConfig {
    pub listen_addr: String,
    pub jwt_secret: String,
    #[serde(default)]
    pub trust_proxy_headers: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    #[serde(default)]
    pub mode: TlsMode,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    #[default]
    Auto,
    Internal,
    External,
    Off,
}

impl TlsConfig {
    pub fn certificates_present(&self) -> bool {
        self.cert_path.is_file() && self.key_path.is_file()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DkimConfig {
    pub key_size: u32,
    pub selector: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginsConfig {
    pub enabled: bool,
    #[serde(default)]
    pub paths: Vec<String>,
    pub directory: Option<String>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        if !std::path::Path::new(path).exists() {
            tracing::info!("Config file not found, using defaults");
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        config.apply_env_overrides();
        config.harden_runtime_secrets();
        Ok(config)
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(secret) = std::env::var("KURIA_JWT_SECRET")
            && !secret.trim().is_empty()
        {
            self.web.jwt_secret = secret;
        }
    }

    fn harden_runtime_secrets(&mut self) {
        if is_insecure_jwt_secret(&self.web.jwt_secret) {
            tracing::warn!(
                "web.jwt_secret is missing, too short, or still set to the default value; \
                 using a temporary random JWT secret for this process. Set KURIA_JWT_SECRET \
                 or web.jwt_secret to a stable random value to keep sessions valid after restart."
            );
            self.web.jwt_secret = generate_ephemeral_jwt_secret();
        }
    }
}

fn is_insecure_jwt_secret(secret: &str) -> bool {
    let secret = secret.trim();
    secret.is_empty() || secret == DEFAULT_JWT_SECRET || secret.len() < MIN_JWT_SECRET_LEN
}

fn generate_ephemeral_jwt_secret() -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use rand_core::{OsRng, RngCore};

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn effective_hostname(config: &Config, db: &sqlx::SqlitePool) -> String {
    crate::db::queries::get_system_setting(db, HOSTNAME_SETTING_KEY)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| config.server.hostname.clone())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                hostname: "localhost".to_string(),
                data_dir: PathBuf::from("./data"),
            },
            smtp: SmtpConfig {
                listen_addr: "0.0.0.0:25".to_string(),
                listen_addr_tls: "0.0.0.0:465".to_string(),
                enable_starttls: true,
            },
            imap: ImapConfig {
                listen_addr: "0.0.0.0:143".to_string(),
                listen_addr_tls: "0.0.0.0:993".to_string(),
                enable_starttls: true,
            },
            pop3: Pop3Config {
                listen_addr: "0.0.0.0:110".to_string(),
                listen_addr_tls: "0.0.0.0:995".to_string(),
            },
            web: WebConfig {
                listen_addr: "0.0.0.0:8080".to_string(),
                jwt_secret: DEFAULT_JWT_SECRET.to_string(),
                trust_proxy_headers: false,
            },
            database: DatabaseConfig {
                url: "sqlite:./data/kuria.db?mode=rwc".to_string(),
            },
            tls: TlsConfig {
                mode: TlsMode::Auto,
                cert_path: PathBuf::from("./data/certs/cert.pem"),
                key_path: PathBuf::from("./data/certs/key.pem"),
            },
            dkim: DkimConfig {
                key_size: 2048,
                selector: "kuria".to_string(),
            },
            plugins: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insecure_jwt_secrets_are_detected() {
        assert!(is_insecure_jwt_secret(""));
        assert!(is_insecure_jwt_secret(DEFAULT_JWT_SECRET));
        assert!(is_insecure_jwt_secret("short"));
        assert!(!is_insecure_jwt_secret("0123456789abcdef0123456789abcdef"));
    }

    #[test]
    fn harden_runtime_secrets_replaces_default_jwt_secret() {
        let mut config = Config::default();
        config.harden_runtime_secrets();

        assert_ne!(config.web.jwt_secret, DEFAULT_JWT_SECRET);
        assert!(!is_insecure_jwt_secret(&config.web.jwt_secret));
    }

    #[test]
    fn harden_runtime_secrets_keeps_strong_jwt_secret() {
        let mut config = Config::default();
        config.web.jwt_secret = "0123456789abcdef0123456789abcdef".to_string();
        config.harden_runtime_secrets();

        assert_eq!(config.web.jwt_secret, "0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn plugin_paths_default_to_empty_when_omitted() {
        let plugins: PluginsConfig = toml::from_str(
            r#"
enabled = true
directory = "./plugins"
"#,
        )
        .expect("plugins config should parse without paths");

        assert!(plugins.paths.is_empty());
        assert_eq!(plugins.directory.as_deref(), Some("./plugins"));
    }
}
