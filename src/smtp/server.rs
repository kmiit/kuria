use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, error};

use crate::config::Config;
use super::session::handle_smtp_connection;

pub struct SmtpServer {
    config: Arc<Config>,
    db: sqlx::SqlitePool,
}

impl SmtpServer {
    pub fn new(config: Arc<Config>, db: sqlx::SqlitePool) -> Self {
        Self { config, db }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.config.smtp.listen_addr).await?;
        info!("SMTP server listening on {}", self.config.smtp.listen_addr);

        loop {
            let (stream, addr) = listener.accept().await?;
            let config = self.config.clone();
            let db = self.db.clone();
            let peer_addr = addr.to_string();

            tokio::spawn(async move {
                info!("SMTP connection from {}", peer_addr);
                if let Err(e) = handle_smtp_connection(stream, config, db, peer_addr.clone()).await {
                    error!("SMTP session error from {}: {}", peer_addr, e);
                }
            });
        }
    }
}
