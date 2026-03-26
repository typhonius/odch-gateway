use serde::Serialize;

/// A user row from the `users` table.
///
/// Field names match the v4 ODCHBot schema (UserStore.pm).
/// Integer timestamps are Unix epoch seconds.
#[derive(Debug, Clone, Serialize)]
pub struct UserRecord {
    pub nick: String,
    pub ip: String,
    pub share: i64,
    pub description: String,
    pub email: String,
    pub speed: String,
    pub connect_time: Option<i64>,
    pub disconnect_time: Option<i64>,
    pub permissions: i64,
}

/// A row from the `history` table (v4 chat log).
#[derive(Debug, Clone, Serialize)]
pub struct ChatHistoryEntry {
    pub id: i64,
    pub nickname: String,
    pub chat: String,
    pub timestamp: i64,
}

/// A row from the `watchdog` / `stats` table (periodic hub snapshots).
///
/// The v3 schema calls this table `watchdog`; v4 calls it `stats`.
/// We accept either table name at query time.
#[derive(Debug, Clone, Serialize)]
pub struct WatchdogEntry {
    pub id: i64,
    pub users_online: i64,
    pub total_share: i64,
    pub timestamp: i64,
}
