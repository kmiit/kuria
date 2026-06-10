mod config;
mod db;
mod error;
mod imap;
mod mail;
mod mail_services;
mod plugin;
mod smtp;
mod tls;
mod web;

use clap::Parser;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use config::Config;
use tls::config::InternalTlsStatus;

/// Handle to the Vite dev server child process (debug mode only).
static VITE_CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);

#[derive(Parser)]
#[command(name = "kuria")]
#[command(about = "A lightweight, self-hosted email server written in Rust")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Initialize database and create admin user
    #[arg(long)]
    init: bool,

    /// Admin email for initialization
    #[arg(long)]
    admin_email: Option<String>,

    /// Admin password for initialization
    #[arg(long)]
    admin_password: Option<String>,
}

/// Spawn the Vite dev server (`npm run dev`) in the frontend directory.
/// In debug mode this gives us hot-reload for the Vue frontend.
fn start_vite_dev_server() {
    use std::process::Command;

    // Detect package manager: prefer bun if available
    let has_bun = Command::new("bun")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let (program, args) = if has_bun {
        ("bun", vec!["run", "dev"])
    } else {
        ("npm", vec!["run", "dev"])
    };

    tracing::info!(
        "Starting Vite dev server ({} {}) ...",
        program,
        args.join(" ")
    );

    match Command::new(program)
        .args(&args)
        .current_dir("frontend")
        .spawn()
    {
        Ok(child) => {
            *VITE_CHILD.lock().unwrap() = Some(child);
            tracing::info!("Vite dev server started — frontend at http://localhost:3000");
        }
        Err(e) => {
            tracing::error!(
                "Failed to start Vite dev server: {}. Run `cd frontend && npm install` first.",
                e
            );
        }
    }
}

struct BoundListeners {
    smtp: TcpListener,
    smtps: Option<TcpListener>,
    imap: TcpListener,
    imaps: Option<TcpListener>,
    internal_tls: InternalTlsStatus,
}

pub use mail_services::listener_disabled;

fn bind_error_hint(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => "端口已被占用",
        std::io::ErrorKind::PermissionDenied => "权限不足，可能需要管理员权限或更换端口",
        std::io::ErrorKind::AddrNotAvailable => "监听地址在本机不可用",
        _ => "无法监听该地址",
    }
}

async fn bind_endpoint(label: &str, addr: &str, errors: &mut Vec<String>) -> Option<TcpListener> {
    match TcpListener::bind(addr).await {
        Ok(listener) => Some(listener),
        Err(error) => {
            errors.push(format!(
                "{} ({})：{} ({})",
                label,
                addr,
                bind_error_hint(&error),
                error
            ));
            None
        }
    }
}

async fn bind_service_listeners(config: &Config) -> anyhow::Result<BoundListeners> {
    let internal_tls = tls::config::internal_tls_status(&config.tls);
    if matches!(internal_tls, InternalTlsStatus::MissingCertificates) {
        anyhow::bail!(
            "TLS mode is internal but certificates were not found at {:?} / {:?}",
            config.tls.cert_path,
            config.tls.key_path
        );
    }

    let internal_tls_enabled = internal_tls.is_enabled();
    let mut errors = Vec::new();

    let smtp = bind_endpoint("SMTP", &config.smtp.listen_addr, &mut errors).await;
    let smtps = if !listener_disabled(&config.smtp.listen_addr_tls) && internal_tls_enabled {
        bind_endpoint("SMTPS", &config.smtp.listen_addr_tls, &mut errors).await
    } else {
        None
    };

    let imap = bind_endpoint("IMAP", &config.imap.listen_addr, &mut errors).await;
    let imaps = if !listener_disabled(&config.imap.listen_addr_tls) && internal_tls_enabled {
        bind_endpoint("IMAPS", &config.imap.listen_addr_tls, &mut errors).await
    } else {
        None
    };

    if !errors.is_empty() {
        anyhow::bail!(
            "端口占用检测失败，Kuria 未启动任何服务。\n{}\n请停止占用进程，或修改 config.toml 中对应的 listen_addr。",
            errors.join("\n")
        );
    }

    Ok(BoundListeners {
        smtp: smtp.expect("SMTP listener must be bound when no errors were reported"),
        smtps,
        imap: imap.expect("IMAP listener must be bound when no errors were reported"),
        imaps,
        internal_tls,
    })
}

fn tls_listener_summary(addr: &str, listener_enabled: bool, status: InternalTlsStatus) -> String {
    if listener_disabled(addr) {
        return "disabled by listen_addr port 0".to_string();
    }

    match status {
        InternalTlsStatus::Enabled if listener_enabled => addr.to_string(),
        InternalTlsStatus::Enabled => "disabled".to_string(),
        InternalTlsStatus::External => "disabled; TLS is handled by external proxy".to_string(),
        InternalTlsStatus::Off => "disabled by tls.mode".to_string(),
        InternalTlsStatus::AutoMissingCertificates => {
            "disabled; certificates not found in auto mode".to_string()
        }
        InternalTlsStatus::MissingCertificates => "disabled; certificates not found".to_string(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load configuration
    let config = Arc::new(Config::load(&cli.config)?);
    tracing::info!("Starting Kuria Mail Server on {}", config.server.hostname);

    // Ensure data directory exists
    std::fs::create_dir_all(&config.server.data_dir)?;
    std::fs::create_dir_all(config.server.data_dir.join("certs"))?;

    // Initialize database
    let db_url = &config.database.url;
    let db = db::init_pool(db_url).await?;
    db::run_migrations(&db).await?;

    // Handle init mode
    if cli.init {
        let email = cli
            .admin_email
            .ok_or_else(|| anyhow::anyhow!("--admin-email required with --init"))?;
        let password = cli
            .admin_password
            .ok_or_else(|| anyhow::anyhow!("--admin-password required with --init"))?;

        // Create domain from email
        let domain_name = email
            .split('@')
            .next_back()
            .ok_or_else(|| anyhow::anyhow!("Invalid email"))?;
        let domain = match db::queries::get_domain_by_name(&db, domain_name).await? {
            Some(d) => d,
            None => db::queries::create_domain(&db, domain_name).await?,
        };

        // Create admin user
        let password_hash = bcrypt::hash(&password, 10)?;
        let user = db::queries::create_user(&db, &email, &password_hash, domain.id, true).await?;
        tracing::info!("Admin user created: {} (id: {})", user.email, user.id);
        return Ok(());
    }

    // Check if config file exists
    let config_exists = std::path::Path::new(&cli.config).exists();

    // Bind web listener first (always needed)
    let web_listener = TcpListener::bind(&config.web.listen_addr).await?;

    // Only bind mail services if config file exists
    let mail_listeners = if config_exists {
        Some(bind_service_listeners(&config).await?)
    } else {
        tracing::info!("No config file found, skipping mail service ports. Complete setup in Web UI.");
        None
    };

    // Load plugins
    let plugin_manager = Arc::new(plugin::PluginManager::load(&config)?);
    plugin_manager.call_init(&config);

    // Mail services state
    let mail_services = Arc::new(mail_services::MailServices::new());

    // Outbound Queue Worker
    let (queue_notifier, queue_rx) = tokio::sync::mpsc::unbounded_channel();
    mail::delivery::set_queue_notifier(queue_notifier);
    let queue_config = config.clone();
    let queue_db = db.clone();
    tokio::spawn(async move {
        let worker = mail::queue::OutboundQueueWorker::new(queue_config, queue_db, queue_rx);
        worker.run().await;
    });

    // In debug mode, start the Vite dev server for frontend hot-reload.
    // The CARGO env var is set by `cargo run` but not when running the binary directly.
    let is_cargo_run = std::env::var("CARGO").is_ok();
    if cfg!(debug_assertions) && is_cargo_run {
        start_vite_dev_server();
    }

    // SMTP Server
    if let Some(listeners) = mail_listeners {
        let smtp_config = config.clone();
        let smtp_db = db.clone();
        let smtp_plugins = plugin_manager.clone();
        let smtp_listener = listeners.smtp;
        let smtps_enabled = listeners.smtps.is_some();
        let smtps_listener = listeners.smtps;
        let internal_tls = listeners.internal_tls;
        let smtp_running = mail_services.smtp_running.clone();
        tokio::spawn(async move {
            let server = smtp::server::SmtpServer::new(smtp_config, smtp_db, smtp_plugins);
            *smtp_running.write().await = true;
            if let Err(e) = server
                .start_with_listeners(smtp_listener, smtps_listener)
                .await
            {
                tracing::error!("SMTP server error: {}", e);
            }
            *smtp_running.write().await = false;
        });

        // IMAP Server
        let imap_config = config.clone();
        let imap_db = db.clone();
        let imap_listener = listeners.imap;
        let imaps_enabled = listeners.imaps.is_some();
        let imaps_listener = listeners.imaps;
        let imap_running = mail_services.imap_running.clone();
        tokio::spawn(async move {
            let server = imap::server::ImapServer::new(imap_config, imap_db);
            *imap_running.write().await = true;
            if let Err(e) = server
                .start_with_listeners(imap_listener, imaps_listener)
                .await
            {
                tracing::error!("IMAP server error: {}", e);
            }
            *imap_running.write().await = false;
        });

        tracing::info!("Kuria Mail Server started successfully");
        tracing::info!("  SMTP: {}", config.smtp.listen_addr);
        tracing::info!(
            "  SMTPS: {}",
            tls_listener_summary(
                &config.smtp.listen_addr_tls,
                smtps_enabled,
                internal_tls
            )
        );
        tracing::info!("  IMAP: {}", config.imap.listen_addr);
        tracing::info!(
            "  IMAPS: {}",
            tls_listener_summary(
                &config.imap.listen_addr_tls,
                imaps_enabled,
                internal_tls
            )
        );
        tracing::info!("  Web:  http://{}", config.web.listen_addr);
        tracing::info!("  Web:  http://{}", config.web.listen_addr);
        if cfg!(debug_assertions) && is_cargo_run {
            tracing::info!("  Frontend (HMR): http://localhost:3000");
        }
    } else {
        tracing::info!("Web UI started on http://{}", config.web.listen_addr);
        tracing::info!("Mail services will start after completing setup wizard");
    }

    // Web Server
    let web_config = config.clone();
    let web_db = db.clone();
    let web_plugins = plugin_manager.clone();
    let web_addr = config.web.listen_addr.clone();
    let web_mail_services = mail_services.clone();
    tokio::spawn(async move {
        let app = web::router::create_router(web_config, web_db, web_plugins, web_mail_services);
        tracing::info!("Web UI listening on {}", web_addr);
        if let Err(e) = axum::serve(web_listener, app).await {
            tracing::error!("Web server error: {}", e);
        }
    });

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    // Shut down plugins
    plugin_manager.call_shutdown();

    // Kill the Vite dev server if running
    if let Ok(mut guard) = VITE_CHILD.lock()
        && let Some(ref mut child) = *guard
    {
        let _ = child.kill();
        let _ = child.wait();
    }

    Ok(())
}
