use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use super::session::{ImapSessionOptions, handle_imap_connection};
use crate::config::Config;

pub struct ImapServer {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

impl ImapServer {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    pub async fn start_with_listeners(
        &self,
        plain_listener: TcpListener,
        tls_listener: Option<TcpListener>,
    ) -> anyhow::Result<()> {
        info!("IMAP server listening on {}", self.config.imap.listen_addr);

        let config = self.config.clone();
        let db = self.db.clone();

        if !crate::listener_disabled(&self.config.imap.listen_addr_tls) && tls_listener.is_none() {
            warn!(
                "IMAPS disabled: {}",
                crate::tls::config::internal_tls_unavailable_message(&config.tls)
            );
        }

        if tls_listener.is_some() {
            info!(
                "IMAPS server listening on {}",
                self.config.imap.listen_addr_tls
            );
        }

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
                let tls_config = match crate::tls::config::load_internal_tls_config(&config2.tls) {
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
                                        let mut reader = tokio::io::BufReader::new(read_half);
                                        let mut writer = write_half;
                                        if let Err(e) = super::session::handle_imap_session(
                                            &mut reader,
                                            &mut writer,
                                            config,
                                            db,
                                            ImapSessionOptions::new(
                                                peer_addr.clone(),
                                                true,
                                                true,
                                                false,
                                            ),
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
