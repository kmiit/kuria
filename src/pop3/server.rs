use std::sync::Arc;
use tokio::net::TcpListener;
use crate::config::Config;

pub struct Pop3Server {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

impl Pop3Server {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    pub async fn start_with_listeners(
        &self,
        listener: TcpListener,
        tls_listener: Option<TcpListener>,
    ) -> anyhow::Result<()> {
        let addr = listener.local_addr()?;
        tracing::info!("POP3 server listening on {}", addr);

        let plain_handle = {
            let config = self.config.clone();
            let db = self.db.clone();
            tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((stream, addr)) => {
                            let config = config.clone();
                            let db = db.clone();
                            tokio::spawn(async move {
                                if let Err(e) = super::session::handle_connection(stream, config, db, false).await {
                                    tracing::debug!("POP3 connection from {} error: {}", addr, e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("POP3 accept error: {}", e);
                        }
                    }
                }
            })
        };

        if let Some(tls_listener) = tls_listener {
            let config = self.config.clone();
            let db = self.db.clone();
            tokio::spawn(async move {
                loop {
                    match tls_listener.accept().await {
                        Ok((stream, addr)) => {
                            let config = config.clone();
                            let db = db.clone();
                            tokio::spawn(async move {
                                if let Err(e) = super::session::handle_connection(stream, config, db, true).await {
                                    tracing::debug!("POP3S connection from {} error: {}", addr, e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("POP3S accept error: {}", e);
                        }
                    }
                }
            });
        }

        plain_handle.await?;
        Ok(())
    }
}
