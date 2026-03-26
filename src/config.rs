use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub hub: HubConfig,
    pub admin: Option<AdminConfig>,
    pub database: Option<DatabaseConfig>,
    pub auth: AuthConfig,
    pub webhook: Option<WebhookConfig>,
    pub rate_limit: Option<RateLimitConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HubConfig {
    pub host: String,
    pub port: u16,
    pub nickname: String,
    #[serde(default = "default_description")]
    pub description: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub share_size: u64,
    #[serde(default = "default_speed")]
    pub speed: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_secs: u64,
    #[serde(default = "default_max_reconnect_delay")]
    pub max_reconnect_delay_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AdminConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
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

fn default_requests_per_minute() -> u32 {
    10
}

fn default_description() -> String {
    "API Gateway".to_string()
}
fn default_speed() -> String {
    "LAN(T1)".to_string()
}
fn default_reconnect_delay() -> u64 {
    5
}
fn default_max_reconnect_delay() -> u64 {
    300
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

impl AppConfig {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&contents)?;
        Ok(config)
    }
}
