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

use tracing_subscriber::EnvFilter;

use crate::bus::EventBus;
use crate::config::AppConfig;
use crate::state::{AppState, HubState};

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
    let (nmdc_tx, nmdc_rx) = tokio::sync::mpsc::channel::<String>(256);

    let _app_state = AppState {
        config: config.clone(),
        event_bus: event_bus.clone(),
        hub_state: hub_state.clone(),
        nmdc_tx: Arc::new(nmdc_tx),
    };

    // Spawn NMDC client
    let hub_config = config.hub.clone();
    let bus = event_bus.clone();
    let state = hub_state.clone();
    tokio::spawn(async move {
        nmdc::client::run(hub_config, bus, state, nmdc_rx).await;
    });

    // Event logger (temporary, for Phase 3 verification)
    let mut event_rx = event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    tracing::info!("Event: {}", serde_json::to_string(&event).unwrap_or_default());
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Event bus lagged by {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    tracing::info!("Gateway running. Press Ctrl+C to stop.");

    // Wait for shutdown
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    Ok(())
}
