use std::sync::Arc;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use super::session::handle_smtp_session;
use crate::config::Config;

pub struct SmtpServer {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

impl SmtpServer {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        // Plain SMTP listener (port 25)
        let plain_listener = TcpListener::bind(&self.config.smtp.listen_addr).await?;
        info!("SMTP server listening on {}", self.config.smtp.listen_addr);

        let config = self.config.clone();
        let db = self.db.clone();

        // Check if TLS certs exist
        let tls_available = config.tls.cert_path.exists() && config.tls.key_path.exists();

        // TLS SMTP listener (port 465) if TLS is configured and certs exist
        let tls_listener = if self.config.smtp.listen_addr_tls != "0.0.0.0:0" && tls_available {
            match TcpListener::bind(&self.config.smtp.listen_addr_tls).await {
                Ok(l) => {
                    info!(
                        "SMTPS server listening on {}",
                        self.config.smtp.listen_addr_tls
                    );
                    Some(l)
                }
                Err(e) => {
                    error!(
                        "Failed to bind SMTPS listener on {}: {}",
                        self.config.smtp.listen_addr_tls, e
                    );
                    None
                }
            }
        } else {
            if self.config.smtp.listen_addr_tls != "0.0.0.0:0" && !tls_available {
                warn!(
                    "SMTPS disabled: TLS certificates not found at {:?} / {:?}",
                    config.tls.cert_path, config.tls.key_path
                );
            }
            None
        };

        // Spawn plain SMTP handler
        let config1 = config.clone();
        let db1 = db.clone();
        tokio::spawn(async move {
            loop {
                match plain_listener.accept().await {
                    Ok((stream, addr)) => {
                        let config = config1.clone();
                        let db = db1.clone();
                        let peer_addr = addr.to_string();
                        tokio::spawn(async move {
                            info!("SMTP connection from {}", peer_addr);
                            let (read_half, write_half) = tokio::io::split(stream);
                            let reader = BufReader::new(read_half);
                            if let Err(e) = handle_smtp_session(
                                reader,
                                write_half,
                                config,
                                db,
                                peer_addr.clone(),
                                false,
                            )
                            .await
                            {
                                error!("SMTP session error from {}: {}", peer_addr, e);
                            }
                        });
                    }
                    Err(e) => error!("Failed to accept SMTP connection: {}", e),
                }
            }
        });

        // Spawn TLS SMTP handler
        if let Some(tls_listener) = tls_listener {
            let config2 = config.clone();
            let db2 = db.clone();
            tokio::spawn(async move {
                let tls_config = match crate::tls::config::load_tls_config(
                    &config2.tls.cert_path,
                    &config2.tls.key_path,
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to load TLS config for SMTPS: {}", e);
                        return;
                    }
                };
                let acceptor = crate::tls::config::create_tls_acceptor(tls_config);

                loop {
                    match tls_listener.accept().await {
                        Ok((stream, addr)) => {
                            let acceptor = acceptor.clone();
                            let config = config2.clone();
                            let db = db2.clone();
                            let peer_addr = addr.to_string();
                            tokio::spawn(async move {
                                info!("SMTPS connection from {}", peer_addr);
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        let (read_half, write_half) = tokio::io::split(tls_stream);
                                        let reader = BufReader::new(read_half);
                                        if let Err(e) = handle_smtp_session(
                                            reader,
                                            write_half,
                                            config,
                                            db,
                                            peer_addr.clone(),
                                            true,
                                        )
                                        .await
                                        {
                                            error!("SMTPS session error from {}: {}", peer_addr, e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("TLS handshake failed from {}: {}", peer_addr, e)
                                    }
                                }
                            });
                        }
                        Err(e) => error!("Failed to accept SMTPS connection: {}", e),
                    }
                }
            });
        }

        // Keep the main task alive
        std::future::pending::<()>().await;
        Ok(())
    }
}
