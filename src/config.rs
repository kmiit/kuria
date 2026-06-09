use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const HOSTNAME_SETTING_KEY: &str = "server.hostname";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    pub smtp: SmtpConfig,
    pub imap: ImapConfig,
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    Auto,
    Internal,
    External,
    Off,
}

impl Default for TlsMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl TlsConfig {
    pub fn certificates_present(&self) -> bool {
        self.cert_path.is_file() && self.key_path.is_file()
    }

    pub fn internal_tls_enabled(&self) -> bool {
        match self.mode {
            TlsMode::Auto | TlsMode::Internal => self.certificates_present(),
            TlsMode::External | TlsMode::Off => false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DkimConfig {
    #[allow(dead_code)]
    pub key_size: u32,
    pub selector: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginsConfig {
    pub enabled: bool,
    pub paths: Vec<String>,
    pub directory: Option<String>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|_| include_str!("../config.toml").to_string());
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
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
            web: WebConfig {
                listen_addr: "0.0.0.0:8080".to_string(),
                jwt_secret: "change-me-in-production".to_string(),
                trust_proxy_headers: false,
            },
            database: DatabaseConfig {
                url: "sqlite:./data/kuria.db".to_string(),
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
