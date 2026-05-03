use chrono::{DateTime, Utc};
use serde::Serialize;

/// A user record from the gateway-owned `users` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct UserRecord {
    pub id: i32,
    pub nick: String,
    pub email: String,
    pub permission: i16,
    pub share_size: i64,
    pub description: String,
    pub speed: String,
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

/// A chat message from the `chat_messages` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ChatMessage {
    pub id: i64,
    pub nick: String,
    pub message: String,
    pub created_at: Option<DateTime<Utc>>,
}

/// A stats snapshot from the `stats_snapshots` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct StatsSnapshot {
    pub id: i32,
    pub user_count: i32,
    pub total_share: i64,
    pub created_at: Option<DateTime<Utc>>,
}

/// A ban record from the `bans` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BanRecord {
    pub id: i32,
    pub nick: Option<String>,
    pub ip: Option<String>,
    pub reason: String,
    pub banned_by: String,
    pub created_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// A tell (offline message) from the `tells` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TellRecord {
    pub id: i32,
    pub from_nick: String,
    pub to_nick: String,
    pub message: String,
    pub created_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
}

/// A quote from the `quotes` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct QuoteRecord {
    pub id: i32,
    pub nick: String,
    pub quote_text: String,
    pub added_by: String,
    pub created_at: Option<DateTime<Utc>>,
}

/// A watch entry from the `watches` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WatchRecord {
    pub id: i32,
    pub watcher_nick: String,
    pub watched_nick: String,
    pub created_at: Option<DateTime<Utc>>,
}

/// A gag record from the `gags` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct GagRecord {
    pub id: i32,
    pub nick: String,
    pub reason: String,
    pub gagged_by: String,
    pub created_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// A bot command from the `bot_commands` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BotCommandRecord {
    pub name: String,
    pub description: String,
    pub aliases: String,
    pub permission: i16,
    pub enabled: bool,
}

/// A key-value entry from the `bot_data` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BotDataEntry {
    pub namespace: String,
    pub key: String,
    pub value: String,
    pub updated_at: Option<DateTime<Utc>>,
}
