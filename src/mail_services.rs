use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MailServices {
    pub smtp_running: Arc<RwLock<bool>>,
    pub imap_running: Arc<RwLock<bool>>,
    pub pop3_running: Arc<RwLock<bool>>,
}

impl MailServices {
    pub fn new() -> Self {
        Self {
            smtp_running: Arc::new(RwLock::new(false)),
            imap_running: Arc::new(RwLock::new(false)),
            pop3_running: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn is_running(&self) -> bool {
        *self.smtp_running.read().await && *self.imap_running.read().await && *self.pop3_running.read().await
    }
}

pub fn listener_disabled(addr: &str) -> bool {
    addr.parse::<std::net::SocketAddr>()
        .map(|addr| addr.port() == 0)
        .unwrap_or(false)
}
