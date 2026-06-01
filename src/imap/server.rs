use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use super::session::handle_imap_connection;
use crate::config::Config;

pub struct ImapServer {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

impl ImapServer {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        // Plain IMAP listener (port 143)
        let plain_listener = TcpListener::bind(&self.config.imap.listen_addr).await?;
        info!("IMAP server listening on {}", self.config.imap.listen_addr);

        let config = self.config.clone();
        let db = self.db.clone();

        // Check if TLS certs exist
        let tls_available = config.tls.cert_path.exists() && config.tls.key_path.exists();

        // TLS IMAP listener (port 993) if configured and certs exist
        let tls_listener = if self.config.imap.listen_addr_tls != "0.0.0.0:0" && tls_available {
            match TcpListener::bind(&self.config.imap.listen_addr_tls).await {
                Ok(l) => {
                    info!(
                        "IMAPS server listening on {}",
                        self.config.imap.listen_addr_tls
                    );
                    Some(l)
                }
                Err(e) => {
                    error!(
                        "Failed to bind IMAPS listener on {}: {}",
                        self.config.imap.listen_addr_tls, e
                    );
                    None
                }
            }
        } else {
            if self.config.imap.listen_addr_tls != "0.0.0.0:0" && !tls_available {
                warn!(
                    "IMAPS disabled: TLS certificates not found at {:?} / {:?}",
                    config.tls.cert_path, config.tls.key_path
                );
            }
            None
        };

        // Spawn plain IMAP handler
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
                            info!("IMAP connection from {}", peer_addr);
                            if let Err(e) =
                                handle_imap_connection(stream, config, db, peer_addr.clone()).await
                            {
                                error!("IMAP session error from {}: {}", peer_addr, e);
                            }
                        });
                    }
                    Err(e) => error!("Failed to accept IMAP connection: {}", e),
                }
            }
        });

        // Spawn TLS IMAP handler
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
                        error!("Failed to load TLS config for IMAPS: {}", e);
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
                                info!("IMAPS connection from {}", peer_addr);
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        let (read_half, write_half) = tokio::io::split(tls_stream);
                                        let reader = tokio::io::BufReader::new(read_half);
                                        if let Err(e) = super::session::handle_imap_session(
                                            reader,
                                            write_half,
                                            config,
                                            db,
                                            peer_addr.clone(),
                                            true,
                                        )
                                        .await
                                        {
                                            error!("IMAPS session error from {}: {}", peer_addr, e);
                                        }
                                    }
                                    Err(e) => error!(
                                        "IMAPS TLS handshake failed from {}: {}",
                                        peer_addr, e
                                    ),
                                }
                            });
                        }
                        Err(e) => error!("Failed to accept IMAPS connection: {}", e),
                    }
                }
            });
        }

        // Keep the main task alive
        std::future::pending::<()>().await;
        Ok(())
    }
}
