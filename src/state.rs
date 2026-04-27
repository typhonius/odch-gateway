use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::bus::EventBus;
use crate::config::AppConfig;
use crate::db::pool::DbPool;
use crate::webhook::manager::WebhookManager;

/// Live hub user info.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HubUser {
    pub nick: String,
    pub description: String,
    pub speed: String,
    pub email: String,
    pub share: u64,
    pub is_op: bool,
}

/// Live hub state.
#[derive(Debug)]
pub struct HubState {
    pub hub_name: RwLock<String>,
    pub topic: RwLock<String>,
    pub users: RwLock<HashMap<String, HubUser>>,
    pub ops: RwLock<Vec<String>>,
    pub connected: RwLock<bool>,
    pub total_share: RwLock<u64>,
    pub uptime_secs: RwLock<u64>,
    pub hub_port: RwLock<u16>,
    pub tls_port: RwLock<u16>,
    pub max_users: RwLock<u32>,
}

impl HubState {
    pub fn new() -> Self {
        Self {
            hub_name: RwLock::new(String::new()),
            topic: RwLock::new(String::new()),
            users: RwLock::new(HashMap::new()),
            ops: RwLock::new(Vec::new()),
            connected: RwLock::new(false),
            total_share: RwLock::new(0),
            uptime_secs: RwLock::new(0),
            hub_port: RwLock::new(0),
            tls_port: RwLock::new(0),
            max_users: RwLock::new(0),
        }
    }
}

impl Default for HubState {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub event_bus: Arc<EventBus>,
    pub hub_state: Arc<HubState>,
    pub nmdc_tx: Arc<tokio::sync::mpsc::Sender<String>>,
    pub admin_tx: Arc<tokio::sync::mpsc::Sender<String>>,
    pub db_pool: Option<DbPool>,
    pub webhook_manager: Arc<WebhookManager>,
    pub ws_connections: Arc<AtomicUsize>,
}
