use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub smtp: SmtpConfig,
    pub imap: ImapConfig,
    pub web: WebConfig,
    pub database: DatabaseConfig,
    pub tls: TlsConfig,
    pub dkim: DkimConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub hostname: String,
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpConfig {
    pub listen_addr: String,
    pub listen_addr_tls: String,
    pub enable_starttls: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImapConfig {
    pub listen_addr: String,
    pub listen_addr_tls: String,
    pub enable_starttls: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebConfig {
    pub listen_addr: String,
    pub jwt_secret: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DkimConfig {
    #[allow(dead_code)]
    pub key_size: u32,
    pub selector: String,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|_| include_str!("../config.toml").to_string());
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
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
            },
            database: DatabaseConfig {
                url: "sqlite:./data/kuria.db".to_string(),
            },
            tls: TlsConfig {
                cert_path: PathBuf::from("./data/certs/cert.pem"),
                key_path: PathBuf::from("./data/certs/key.pem"),
            },
            dkim: DkimConfig {
                key_size: 2048,
                selector: "kuria".to_string(),
            },
        }
    }
}
