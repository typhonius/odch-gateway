use serde::Deserialize;
use std::fmt;

#[derive(Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub admin: AdminConfig,
    pub database: Option<DatabaseConfig>,
    pub auth: AuthConfig,
    pub webhook: Option<WebhookConfig>,
    pub rate_limit: Option<RateLimitConfig>,
    pub admin_ui: Option<AdminUiConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

#[derive(Deserialize, Clone)]
pub struct AdminConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
}

#[derive(Deserialize, Clone)]
pub struct DatabaseConfig {
    /// Connection URL. Examples:
    ///   sqlite:///path/to/odchbot.db?mode=ro
    ///   postgres://user:pass@localhost:5432/odchbot
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
    #[serde(default = "default_storage_path")]
    pub storage_path: String,
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
fn default_storage_path() -> String {
    "webhooks.json".to_string()
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppConfig")
            .field("server", &self.server)
            .field("admin", &format!("AdminConfig {{ host: {:?}, port: {} }}", self.admin.host, self.admin.port))
            .field("database", &"[REDACTED]")
            .field("auth", &format!("[{} key(s)]", self.auth.api_keys.len()))
            .field("admin_ui", &self.admin_ui.as_ref().map(|a| format!("AdminUiConfig {{ bind: {:?} }}", a.bind_address)))
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
