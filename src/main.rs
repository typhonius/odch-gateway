mod admin_ui;
mod api;
mod bot;
mod bus;
mod config;
mod db;
mod error;
mod event;
mod hub;
mod init;
mod state;
mod webhook;

use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::bus::EventBus;
use crate::config::AppConfig;
use crate::state::{AppState, HubState};
use crate::webhook::manager::WebhookManager;

#[derive(Parser)]
#[command(
    name = "odch-gateway",
    version,
    about = "REST/WebSocket API gateway for OpenDCHub"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to config file (default: config.toml)
    #[arg(global = true, default_value = "config.toml")]
    config: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Set up the entire stack: create DB, generate secrets, write configs
    Init {
        /// PostgreSQL host
        #[arg(long, default_value = "localhost")]
        db_host: String,
        /// PostgreSQL port
        #[arg(long, default_value_t = 5432)]
        db_port: u16,
        /// Database name
        #[arg(long, default_value = "odch")]
        db_name: String,
        /// Database user
        #[arg(long, default_value = "odch")]
        db_user: String,
        /// Database password
        #[arg(long, default_value = "odch")]
        db_password: String,
        /// Hub NMDC port
        #[arg(long, default_value_t = 4012)]
        hub_port: u16,
        /// Gateway API port
        #[arg(long, default_value_t = 3000)]
        api_port: u16,
        /// Admin UI port
        #[arg(long, default_value_t = 3001)]
        admin_ui_port: u16,
        /// Config directory
        #[arg(long, default_value = "/opt/opendchub")]
        config_dir: String,
        /// Create systemd service files
        #[arg(long)]
        systemd: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Handle init subcommand
    if let Some(Commands::Init {
        db_host,
        db_port,
        db_name,
        db_user,
        db_password,
        hub_port,
        api_port,
        admin_ui_port,
        config_dir,
        systemd,
    }) = cli.command
    {
        return init::run_init(init::InitConfig {
            db_host: &db_host,
            db_port,
            db_name: &db_name,
            db_user: &db_user,
            db_password: &db_password,
            hub_port,
            api_port,
            admin_ui_port,
            config_dir: &config_dir,
            install_services: systemd,
        });
    }

    // Normal server mode
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config_path = cli.config;
    let config = AppConfig::load(&config_path)?;
    let config = Arc::new(config);

    tracing::info!("odch-gateway v{}", env!("CARGO_PKG_VERSION"));

    // Set up shared state
    let event_bus = Arc::new(EventBus::new(1024));
    let hub_state = Arc::new(HubState::new());
    let (admin_tx, admin_rx) = tokio::sync::mpsc::channel::<String>(256);

    // Set up database pool (optional)
    let db_pool = match &config.database {
        Some(db_config) => match db::pool::create_pool(&db_config.url).await {
            Ok(pool) => {
                // Redact credentials from URL for logging
                let safe_url = db_config
                    .url
                    .find('@')
                    .map(|i| &db_config.url[i + 1..])
                    .unwrap_or("configured");
                tracing::info!("Database pool created for: {}", safe_url);
                Some(pool)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to create database pool: {}. Continuing without DB.",
                    e
                );
                None
            }
        },
        None => {
            tracing::info!("No database configured");
            None
        }
    };

    // Set up webhook manager (database-backed)
    let webhook_config = config
        .webhook
        .clone()
        .unwrap_or(crate::config::WebhookConfig {
            max_retries: 3,
            retry_delay_secs: 5,
            timeout_secs: 10,
            max_webhooks: 50,
        });
    let webhook_manager = Arc::new(WebhookManager::new(
        db_pool.as_ref().map(|p| p.inner().clone()),
        webhook_config.max_webhooks,
    ));

    // Create command engine (if DB is configured)
    let command_engine = if db_pool.is_some() {
        Some(Arc::new(bot::CommandEngine::new()))
    } else {
        None
    };

    let app_state = AppState {
        config: config.clone(),
        event_bus: event_bus.clone(),
        hub_state: hub_state.clone(),
        admin_tx: Arc::new(admin_tx),
        db_pool,
        webhook_manager: webhook_manager.clone(),
        ws_connections: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        command_engine: command_engine.clone(),
    };

    // Connect to hub via Unix socket
    let hub_config = config
        .hub
        .clone()
        .ok_or("No [hub] section in config. Set socket_path and secret.")?;
    tracing::info!(
        "Connecting to hub via Unix socket: {}",
        hub_config.socket_path
    );
    {
        let bus = event_bus.clone();
        let state = hub_state.clone();
        tokio::spawn(async move {
            hub::socket::run(hub_config, bus, state, admin_rx).await;
        });
    }

    // Spawn event processor (stores events in DB, delivers tells, notifies watchers)
    if let Some(ref pool) = app_state.db_pool {
        let ep_bus = event_bus.clone();
        let ep_pool = pool.clone();
        let ep_tx = app_state.admin_tx.as_ref().clone();
        tokio::spawn(async move {
            db::event_processor::run(ep_bus, ep_pool, ep_tx).await;
        });
    }

    // Spawn built-in bot command processor
    if let (Some(ref pool), Some(ref engine)) = (&app_state.db_pool, &command_engine) {
        let bot_bus = event_bus.clone();
        let bot_pool = pool.inner().clone();
        let bot_tx = app_state.admin_tx.clone();
        let bot_engine = engine.clone();
        tokio::spawn(async move {
            let mut rx = bot_bus.subscribe();

            loop {
                match rx.recv().await {
                    Ok(crate::event::HubEvent::Chat {
                        ref nick,
                        ref message,
                        ..
                    }) => {
                        let tx: tokio::sync::mpsc::Sender<String> = (*bot_tx).clone();
                        if let Some(response) = bot_engine
                            .try_handle(nick, message, bot_pool.clone(), tx.clone())
                            .await
                        {
                            bot_engine.send_response(response, nick, &tx).await;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Bot command processor lagged by {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    // Spawn webhook dispatcher
    {
        let wh_rx = event_bus.subscribe();
        let wh_mgr = webhook_manager.clone();
        let wh_cfg = webhook_config.clone();
        tokio::spawn(async move {
            webhook::delivery::run_dispatcher(wh_mgr, wh_rx, wh_cfg).await;
        });
    }

    // Event logger
    let mut event_rx = event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    tracing::info!(
                        "Event: {}",
                        serde_json::to_string(&event).unwrap_or_default()
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Event bus lagged by {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Build HTTP router and start server
    let router = api::build_router(app_state.clone());
    let cancel_token = CancellationToken::new();

    let bind_addr = &config.server.bind_address;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("API server listening on {}", bind_addr);

    // Main API server
    let main_token = cancel_token.clone();
    let main_handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(main_token.cancelled_owned())
            .await
    });

    // Admin UI server (if configured)
    if let Some(ref admin_ui_config) = config.admin_ui {
        let admin_router = admin_ui::build_admin_router(app_state);
        let admin_listener = tokio::net::TcpListener::bind(&admin_ui_config.bind_address).await?;
        tracing::info!("Admin UI listening on {}", admin_ui_config.bind_address);
        let admin_token = cancel_token.clone();
        tokio::spawn(async move {
            if let Err(e) = axum::serve(admin_listener, admin_router)
                .with_graceful_shutdown(admin_token.cancelled_owned())
                .await
            {
                tracing::error!("Admin UI server error: {e}");
            }
        });
    }

    // Wait for shutdown signal, then cancel both servers
    shutdown_signal().await;
    cancel_token.cancel();
    main_handle.await??;

    Ok(())
}

/// Wait for either SIGINT (Ctrl+C) or SIGTERM, then return.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl+c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown...");
}
