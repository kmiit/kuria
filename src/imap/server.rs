use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, error};

use crate::config::Config;
use super::session::handle_imap_connection;

pub struct ImapServer {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

impl ImapServer {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.config.imap.listen_addr).await?;
        info!("IMAP server listening on {}", self.config.imap.listen_addr);

        loop {
            let (stream, addr) = listener.accept().await?;
            let config = self.config.clone();
            let db = self.db.clone();
            let peer_addr = addr.to_string();

            tokio::spawn(async move {
                info!("IMAP connection from {}", peer_addr);
                if let Err(e) = handle_imap_connection(stream, config, db, peer_addr.clone()).await {
                    error!("IMAP session error from {}: {}", peer_addr, e);
                }
            });
        }
    }
}
