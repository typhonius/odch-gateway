//! Event processor that receives hub events and stores them in the database.
//!
//! Also handles reactive behaviors:
//! - User joins → deliver pending tells, notify watchers
//! - User quits → notify watchers, close session
//! - Chat → store in DB

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::bus::EventBus;
use crate::db::pool::DbPool;
use crate::db::queries;
use crate::event::HubEvent;
use crate::state::HubState;

/// Run the event processor loop. Should be spawned as a tokio task.
pub async fn run(
    event_bus: Arc<EventBus>,
    db_pool: DbPool,
    hub_tx: mpsc::Sender<String>,
    hub_state: Arc<HubState>,
) {
    let mut rx = event_bus.subscribe();
    tracing::info!("Event processor started");

    loop {
        match rx.recv().await {
            Ok(event) => {
                if let Err(e) = process_event(&db_pool, &event, &hub_tx, &hub_state).await {
                    tracing::warn!("Event processor error: {}", e);
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Event processor lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                tracing::info!("Event processor shutting down (bus closed)");
                break;
            }
        }
    }
}

async fn process_event(
    pool: &DbPool,
    event: &HubEvent,
    hub_tx: &mpsc::Sender<String>,
    hub_state: &Arc<HubState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = pool.inner();

    match event {
        HubEvent::Chat { nick, message, .. } => {
            queries::insert_chat(db, nick, message).await?;
            queries::upsert_user(db, nick, "", 0, "", "").await.ok();
        }

        HubEvent::UserJoin { nick, .. } => {
            // Check if user is banned — kick immediately if so
            if let Ok(Some(_ban)) = queries::check_ban(db, nick).await {
                let kick_cmd = serde_json::json!({"type": "kick", "nick": nick});
                let _ = hub_tx.send(kick_cmd.to_string()).await;
                tracing::info!("Kicked banned user {} on join", nick);
                return Ok(());
            }

            let user_id = queries::upsert_user(db, nick, "", 0, "", "").await?;
            queries::open_session(db, user_id, "", false).await.ok();

            // Deliver pending tells
            if let Ok(tells) = queries::get_pending_tells(db, nick).await {
                for tell in &tells {
                    let msg = format!(
                        "Tell from {} ({}): {}",
                        tell.from_nick,
                        tell.created_at
                            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_default(),
                        tell.message
                    );
                    let cmd = serde_json::json!({
                        "type": "send_to",
                        "nick": nick,
                        "message": msg,
                    });
                    let _ = hub_tx.send(cmd.to_string()).await;
                    queries::mark_tell_delivered(db, tell.id).await.ok();
                }
            }

            // Notify watchers
            if let Ok(watchers) = queries::get_watchers(db, nick).await {
                for w in &watchers {
                    let cmd = serde_json::json!({
                        "type": "send_to",
                        "nick": w.watcher_nick,
                        "message": format!("{} has logged in.", nick),
                    });
                    let _ = hub_tx.send(cmd.to_string()).await;
                }
            }
        }

        HubEvent::UserQuit { nick, .. } => {
            queries::close_sessions(db, nick).await?;
            queries::update_last_seen(db, nick).await.ok();

            // Notify watchers
            if let Ok(watchers) = queries::get_watchers(db, nick).await {
                for w in &watchers {
                    let cmd = serde_json::json!({
                        "type": "send_to",
                        "nick": w.watcher_nick,
                        "message": format!("{} has logged out.", nick),
                    });
                    let _ = hub_tx.send(cmd.to_string()).await;
                }
            }
        }

        HubEvent::UserInfo {
            nick,
            description,
            speed,
            email,
            share,
            ..
        } => {
            queries::upsert_user(db, nick, email, *share as i64, description, speed).await?;
        }

        HubEvent::MaintenanceTick { .. } => {
            // Purge stale connections (users stuck in NMDC handshake)
            let cmd = serde_json::json!({"type": "purge_stale"});
            let _ = hub_tx.send(cmd.to_string()).await;

            // Purge expired bans and gags
            if let Ok(n) = queries::purge_expired_bans(db).await {
                if n > 0 {
                    tracing::info!("Purged {} expired ban(s)", n);
                }
            }
            if let Ok(n) = queries::purge_expired_gags(db).await {
                if n > 0 {
                    tracing::info!("Purged {} expired gag(s)", n);
                }
            }

            // Close orphaned sessions (users no longer online but session still open)
            let online_nicks: Vec<String> =
                hub_state.users.read().await.keys().cloned().collect();
            if let Ok(n) = queries::close_orphaned_sessions(db, &online_nicks).await {
                if n > 0 {
                    tracing::info!("Closed {} orphaned session(s)", n);
                }
            }
        }

        _ => {}
    }

    Ok(())
}
