use chrono::{DateTime, Utc};
use serde::Serialize;

/// Hub events that flow through the event bus.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum HubEvent {
    Chat {
        nick: String,
        message: String,
        timestamp: DateTime<Utc>,
    },
    UserJoin {
        nick: String,
        timestamp: DateTime<Utc>,
    },
    UserQuit {
        nick: String,
        timestamp: DateTime<Utc>,
    },
    UserInfo {
        nick: String,
        description: String,
        speed: String,
        email: String,
        share: u64,
        timestamp: DateTime<Utc>,
    },
    HubName {
        name: String,
        timestamp: DateTime<Utc>,
    },
    OpListUpdate {
        ops: Vec<String>,
        timestamp: DateTime<Utc>,
    },
    Kick {
        nick: String,
        by: String,
        timestamp: DateTime<Utc>,
    },
    PrivateMessage {
        from: String,
        to: String,
        message: String,
        timestamp: DateTime<Utc>,
    },
    GatewayStatus {
        connected: bool,
        message: String,
        timestamp: DateTime<Utc>,
    },
    Ban {
        nick: String,
        by: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    Unban {
        nick: String,
        by: String,
        timestamp: DateTime<Utc>,
    },
    Gag {
        nick: String,
        by: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    Ungag {
        nick: String,
        by: String,
        timestamp: DateTime<Utc>,
    },
    /// Periodic maintenance tick. Fired by a configurable timer.
    /// Subscribers use this to perform housekeeping (purge stale
    /// connections, expire bans/gags, etc.).
    MaintenanceTick {
        timestamp: DateTime<Utc>,
    },
}
