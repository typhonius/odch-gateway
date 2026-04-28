mod admin_ui;
mod api;
mod bus;
mod config;
mod db;
mod error;
mod event;
mod nmdc;
mod state;
mod webhook;

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::bus::EventBus;
use crate::config::AppConfig;
use crate::state::{AppState, HubState};
use crate::webhook::manager::WebhookManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

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

    // Set up webhook manager
    let webhook_config = config
        .webhook
        .clone()
        .unwrap_or_else(|| crate::config::WebhookConfig {
            max_retries: 3,
            retry_delay_secs: 5,
            timeout_secs: 10,
            max_webhooks: 50,
            storage_path: "webhooks.json".to_string(),
        });
    let webhook_manager = Arc::new(WebhookManager::new(
        &webhook_config.storage_path,
        webhook_config.max_webhooks,
    ));

    let app_state = AppState {
        config: config.clone(),
        event_bus: event_bus.clone(),
        hub_state: hub_state.clone(),
        admin_tx: Arc::new(admin_tx),
        db_pool,
        webhook_manager: webhook_manager.clone(),
        ws_connections: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    // The gateway operates entirely through the admin port — no NMDC client
    // connection needed. The admin port provides: event stream, status data,
    // user list, moderation commands, $DataToAll for chat, and user registration.
    {
        let admin_config = config.admin.clone();
        let bus = event_bus.clone();
        let state = hub_state.clone();
        tokio::spawn(async move {
            nmdc::admin::run(admin_config, bus, state, admin_rx).await;
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
        let admin_listener =
            tokio::net::TcpListener::bind(&admin_ui_config.bind_address).await?;
        tracing::info!(
            "Admin UI listening on {}",
            admin_ui_config.bind_address
        );
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
