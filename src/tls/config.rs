use rustls::pki_types::CertificateDer;
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;
use tracing::info;

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

/// Create a TLS acceptor
pub fn create_tls_acceptor(config: Arc<rustls::ServerConfig>) -> TlsAcceptor {
    TlsAcceptor::from(config)
}
