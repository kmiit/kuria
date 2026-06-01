mod config;
mod db;
mod dns;
mod error;
mod imap;
mod mail;
mod smtp;
mod tls;
mod web;

use clap::Parser;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

use config::Config;

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

    // In debug mode, start the Vite dev server for frontend hot-reload.
    // The CARGO env var is set by `cargo run` but not when running the binary directly.
    let is_cargo_run = std::env::var("CARGO").is_ok();
    if cfg!(debug_assertions) && is_cargo_run {
        start_vite_dev_server();
    }

    // Start services

    // SMTP Server
    let smtp_config = config.clone();
    let smtp_db = db.clone();
    tokio::spawn(async move {
        let server = smtp::server::SmtpServer::new(smtp_config, smtp_db);
        if let Err(e) = server.start().await {
            tracing::error!("SMTP server error: {}", e);
        }
    });

    // IMAP Server
    let imap_config = config.clone();
    let imap_db = db.clone();
    tokio::spawn(async move {
        let server = imap::server::ImapServer::new(imap_config, imap_db);
        if let Err(e) = server.start().await {
            tracing::error!("IMAP server error: {}", e);
        }
    });

    // Web Server
    let web_config = config.clone();
    let web_db = db.clone();
    let web_addr = config.web.listen_addr.clone();
    tokio::spawn(async move {
        let app = web::router::create_router(web_config, web_db);
        let listener = tokio::net::TcpListener::bind(&web_addr).await.unwrap();
        tracing::info!("Web UI listening on {}", web_addr);
        axum::serve(listener, app).await.unwrap();
    });

    tracing::info!("Kuria Mail Server started successfully");
    tracing::info!("  SMTP: {}", config.smtp.listen_addr);
    tracing::info!("  IMAP: {}", config.imap.listen_addr);
    tracing::info!("  Web:  http://{}", config.web.listen_addr);
    if cfg!(debug_assertions) && is_cargo_run {
        tracing::info!("  Frontend (HMR): http://localhost:3000");
    }

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    // Kill the Vite dev server if running
    if let Ok(mut guard) = VITE_CHILD.lock()
        && let Some(ref mut child) = *guard
    {
        let _ = child.kill();
        let _ = child.wait();
    }

    Ok(())
}
