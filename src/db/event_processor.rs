//! Event processor that receives hub events and stores them in the database.
//!
//! Spawned as a background task. Listens on the event bus and writes:
//! - Chat messages → chat_messages table
//! - User joins → upsert user + open session
//! - User quits → close session + update last_seen
//! - MyINFO updates → upsert user record

use std::sync::Arc;

use crate::bus::EventBus;
use crate::db::pool::DbPool;
use crate::db::queries;
use crate::event::HubEvent;

/// Run the event processor loop. Should be spawned as a tokio task.
pub async fn run(event_bus: Arc<EventBus>, db_pool: DbPool) {
    let mut rx = event_bus.subscribe();
    tracing::info!("Event processor started");

    loop {
        match rx.recv().await {
            Ok(event) => {
                if let Err(e) = process_event(&db_pool, &event).await {
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

async fn process_event(pool: &DbPool, event: &HubEvent) -> Result<(), Box<dyn std::error::Error>> {
    let db = pool.inner();

    match event {
        HubEvent::Chat { nick, message, .. } => {
            queries::insert_chat(db, nick, message).await?;
            // Also upsert user to track last_seen
            queries::upsert_user(db, nick, "", 0, "", "").await.ok();
        }

        HubEvent::UserJoin { nick, .. } => {
            let user_id = queries::upsert_user(db, nick, "", 0, "", "").await?;
            // Open a session (we don't have IP/TLS info from the event alone,
            // those come with the MyINFO update)
            queries::open_session(db, user_id, "", false).await.ok();
        }

        HubEvent::UserQuit { nick, .. } => {
            queries::close_sessions(db, nick).await?;
            queries::update_last_seen(db, nick).await.ok();
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

        // Hub name, op list, kick, gateway status — no DB writes needed
        _ => {}
    }

    Ok(())
}
