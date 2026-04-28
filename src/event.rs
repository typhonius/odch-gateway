use chrono::{DateTime, Utc};
use serde::Serialize;

/// Hub events that flow through the event bus.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
#[allow(dead_code)]
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
    GatewayStatus {
        connected: bool,
        message: String,
        timestamp: DateTime<Utc>,
    },
}
