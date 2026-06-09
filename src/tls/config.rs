use rustls::pki_types::CertificateDer;
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;
use tracing::info;

use crate::config::TlsConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalTlsStatus {
    Enabled,
    External,
    Off,
    AutoMissingCertificates,
    MissingCertificates,
}

impl InternalTlsStatus {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

pub fn internal_tls_status(config: &TlsConfig) -> InternalTlsStatus {
    match config.mode {
        crate::config::TlsMode::Internal if config.certificates_present() => {
            InternalTlsStatus::Enabled
        }
        crate::config::TlsMode::Internal => InternalTlsStatus::MissingCertificates,
        crate::config::TlsMode::Auto if config.certificates_present() => InternalTlsStatus::Enabled,
        crate::config::TlsMode::Auto => InternalTlsStatus::AutoMissingCertificates,
        crate::config::TlsMode::External => InternalTlsStatus::External,
        crate::config::TlsMode::Off => InternalTlsStatus::Off,
    }
}

pub fn internal_tls_unavailable_message(config: &TlsConfig) -> String {
    match internal_tls_status(config) {
        InternalTlsStatus::Enabled => "internal TLS is enabled".to_string(),
        InternalTlsStatus::External => {
            "TLS mode is external; terminate TLS in Nginx or another proxy".to_string()
        }
        InternalTlsStatus::Off => "TLS mode is off".to_string(),
        InternalTlsStatus::AutoMissingCertificates | InternalTlsStatus::MissingCertificates => {
            format!(
                "TLS certificates not found at {:?} / {:?}",
                config.cert_path, config.key_path
            )
        }
    }
}

/// Load TLS configuration from certificate and key files
pub fn load_tls_config(
    cert_path: &Path,
    key_path: &Path,
) -> anyhow::Result<Arc<rustls::ServerConfig>> {
    let cert_file = std::fs::File::open(cert_path)?;
    let mut cert_reader = std::io::BufReader::new(cert_file);
    let certs: Vec<CertificateDer> =
        rustls_pemfile::certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;

    let key_file = std::fs::File::open(key_path)?;
    let mut key_reader = std::io::BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)?
        .ok_or_else(|| anyhow::anyhow!("No private key found in key file"))?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    info!(
        "TLS configuration loaded from {:?} and {:?}",
        cert_path, key_path
    );
    Ok(Arc::new(config))
}

pub fn load_internal_tls_config(config: &TlsConfig) -> anyhow::Result<Arc<rustls::ServerConfig>> {
    if !internal_tls_status(config).is_enabled() {
        anyhow::bail!("{}", internal_tls_unavailable_message(config));
    }

    load_tls_config(&config.cert_path, &config.key_path)
}

/// Create a TLS acceptor
pub fn create_tls_acceptor(config: Arc<rustls::ServerConfig>) -> TlsAcceptor {
    TlsAcceptor::from(config)
}
