use serde::Deserialize;
use std::fmt;

#[derive(Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    /// Unix socket connection to hub
    pub hub: Option<HubSocketConfig>,
    pub database: Option<DatabaseConfig>,
    pub auth: AuthConfig,
    pub webhook: Option<WebhookConfig>,
    pub rate_limit: Option<RateLimitConfig>,
    pub admin_ui: Option<AdminUiConfig>,
    pub greeting: Option<GreetingConfig>,
    pub opchat: Option<OpChatConfig>,
}

#[derive(Deserialize, Clone)]
pub struct HubSocketConfig {
    /// Path to the Unix domain socket (e.g. "/opt/opendchub/.opendchub/gateway.sock")
    pub socket_path: String,
    /// Shared secret for authentication
    pub secret: String,
    /// Interval in seconds for the maintenance tick (default: 60).
    /// Fires a MaintenanceTick event on the bus for periodic housekeeping.
    #[serde(default = "default_maintenance_interval")]
    pub maintenance_interval_secs: u64,
}

fn default_maintenance_interval() -> u64 {
    60
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Nick used for system messages (command responses, gag notices, etc.).
    /// Must not contain spaces. Default: "Sentinel"
    #[serde(default = "default_system_nick")]
    pub system_nick: String,
}

fn default_system_nick() -> String {
    "Sentinel".to_string()
}

#[derive(Deserialize, Clone)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL. Example:
    ///   postgres://odch:password@localhost:5432/odch
    pub url: String,
}

#[derive(Deserialize, Clone)]
pub struct AuthConfig {
    pub api_keys: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WebhookConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_webhooks")]
    pub max_webhooks: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per minute per API key (default: 10).
    #[serde(default = "default_requests_per_minute")]
    pub requests_per_minute: u32,
}

#[derive(Deserialize, Clone)]
pub struct AdminUiConfig {
    pub bind_address: String,
    pub username: String,
    pub password_hash: String,
    #[serde(default = "default_session_expiry_hours")]
    pub session_expiry_hours: u64,
    pub jwt_secret: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GreetingConfig {
    /// Message sent to users on connect. Supports {hub_name} placeholder.
    /// Default: "Welcome to {hub_name}. Type !help for commands."
    pub welcome_message: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OpChatConfig {
    /// Virtual user nick for OP group chat. Default: "OPChat"
    #[serde(default = "default_opchat_nick")]
    pub nick: String,
    /// Description shown in user list. Default: "OP Group Chat"
    #[serde(default = "default_opchat_description")]
    pub description: String,
}

fn default_opchat_nick() -> String {
    "OPChat".to_string()
}

fn default_opchat_description() -> String {
    "OP Group Chat".to_string()
}

fn default_session_expiry_hours() -> u64 {
    8
}

fn default_requests_per_minute() -> u32 {
    10
}

fn default_max_retries() -> u32 {
    3
}
fn default_retry_delay() -> u64 {
    5
}
fn default_timeout() -> u64 {
    10
}
fn default_max_webhooks() -> usize {
    50
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppConfig")
            .field("server", &self.server)
            .field(
                "hub",
                &self
                    .hub
                    .as_ref()
                    .map(|h| format!("HubSocketConfig {{ path: {:?} }}", h.socket_path)),
            )
            .field("database", &"[REDACTED]")
            .field("auth", &format!("[{} key(s)]", self.auth.api_keys.len()))
            .field(
                "admin_ui",
                &self
                    .admin_ui
                    .as_ref()
                    .map(|a| format!("AdminUiConfig {{ bind: {:?} }}", a.bind_address)),
            )
            .finish()
    }
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&contents)?;
        Ok(config)
    }
}
