use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::db::queries;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct HubInfoResponse {
    pub hub_name: String,
    pub topic: String,
    pub user_count: usize,
    pub op_count: usize,
    pub total_share: u64,
    pub connected: bool,
    pub uptime_secs: u64,
    pub hub_port: u16,
    pub tls_port: u16,
    pub max_users: u32,
    pub gateway_version: &'static str,
}

/// GET /api/hub/info
///
/// Returns live hub information: name, user count, op count, total share, connected status.
pub async fn get_hub_info(
    State(state): State<AppState>,
) -> Result<Json<HubInfoResponse>, AppError> {
    let hub_name = state.hub_state.hub_name.read().await.clone();
    let topic = state.hub_state.topic.read().await.clone();
    let users = state.hub_state.users.read().await;
    let ops = state.hub_state.ops.read().await;
    let total_share = *state.hub_state.total_share.read().await;
    let connected = *state.hub_state.connected.read().await;

    let uptime_secs = *state.hub_state.uptime_secs.read().await;
    let hub_port = *state.hub_state.hub_port.read().await;
    let tls_port = *state.hub_state.tls_port.read().await;
    let max_users = *state.hub_state.max_users.read().await;

    Ok(Json(HubInfoResponse {
        hub_name,
        topic,
        user_count: users.len(),
        op_count: ops.len(),
        total_share,
        connected,
        uptime_secs,
        hub_port,
        tls_port,
        max_users,
        gateway_version: env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    #[serde(default = "default_stats_limit")]
    pub limit: i64,
}

fn default_stats_limit() -> i64 {
    100
}

/// GET /api/hub/stats
///
/// Returns historical hub stats from the DB (watchdog/stats table).
pub async fn get_hub_stats(
    State(state): State<AppState>,
    Query(params): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::Internal("Database not configured".to_string()))?;

    let limit = params.limit.clamp(1, 1000);
    let stats = queries::get_hub_stats(pool, limit).await?;

    Ok(Json(serde_json::json!({
        "stats": stats,
        "count": stats.len(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_stats_limit() {
        assert_eq!(default_stats_limit(), 100);
    }
}
