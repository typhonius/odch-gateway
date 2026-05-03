use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::db::models::*;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

pub async fn get_user(pool: &PgPool, nick: &str) -> Result<Option<UserRecord>, AppError> {
    let user = sqlx::query_as::<_, UserRecord>(
        "SELECT id, nick, email, permission, share_size, description, speed, \
                password_hash, first_seen, last_seen, created_at \
         FROM users WHERE nick = $1",
    )
    .bind(nick)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn get_user_with_password(
    pool: &PgPool,
    nick: &str,
) -> Result<Option<(String, i16)>, AppError> {
    let row = sqlx::query_as::<_, (String, i16)>(
        "SELECT password_hash, permission FROM users \
         WHERE nick = $1 AND password_hash IS NOT NULL",
    )
    .bind(nick)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn upsert_user(
    pool: &PgPool,
    nick: &str,
    email: &str,
    share_size: i64,
    description: &str,
    speed: &str,
) -> Result<i32, AppError> {
    let rec = sqlx::query_scalar::<_, i32>(
        "INSERT INTO users (nick, email, share_size, description, speed, first_seen, last_seen) \
         VALUES ($1, $2, $3, $4, $5, NOW(), NOW()) \
         ON CONFLICT (nick) DO UPDATE SET \
           email = EXCLUDED.email, share_size = EXCLUDED.share_size, \
           description = EXCLUDED.description, speed = EXCLUDED.speed, \
           last_seen = NOW() \
         RETURNING id",
    )
    .bind(nick)
    .bind(email)
    .bind(share_size)
    .bind(description)
    .bind(speed)
    .fetch_one(pool)
    .await?;
    Ok(rec)
}

pub async fn list_registered_users(pool: &PgPool) -> Result<Vec<UserRecord>, AppError> {
    let rows = sqlx::query_as::<_, UserRecord>(
        "SELECT id, nick, email, permission, share_size, description, speed, \
                password_hash, first_seen, last_seen, created_at \
         FROM users WHERE password_hash IS NOT NULL AND permission > 0 \
         ORDER BY nick",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update_last_seen(pool: &PgPool, nick: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET last_seen = NOW() WHERE nick = $1")
        .bind(nick)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

pub async fn open_session(
    pool: &PgPool,
    user_id: i32,
    ip: &str,
    tls: bool,
) -> Result<i32, AppError> {
    let id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO user_sessions (user_id, login_at, ip_address, tls) \
         VALUES ($1, NOW(), $2, $3) RETURNING id",
    )
    .bind(user_id)
    .bind(ip)
    .bind(tls)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn close_sessions(pool: &PgPool, nick: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE user_sessions SET logout_at = NOW() \
         WHERE user_id = (SELECT id FROM users WHERE nick = $1) \
           AND logout_at IS NULL",
    )
    .bind(nick)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

pub async fn insert_chat(pool: &PgPool, nick: &str, message: &str) -> Result<(), AppError> {
    sqlx::query("INSERT INTO chat_messages (nick, message) VALUES ($1, $2)")
        .bind(nick)
        .bind(message)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_chat_history(
    pool: &PgPool,
    limit: i64,
    offset: i64,
) -> Result<Vec<ChatMessage>, AppError> {
    let rows = sqlx::query_as::<_, ChatMessage>(
        "SELECT id, nick, message, created_at \
         FROM chat_messages ORDER BY created_at DESC LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_user_chat_history(
    pool: &PgPool,
    nick: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<ChatMessage>, AppError> {
    let rows = sqlx::query_as::<_, ChatMessage>(
        "SELECT id, nick, message, created_at \
         FROM chat_messages WHERE nick = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(nick)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn search_chat(
    pool: &PgPool,
    query: &str,
    nick: Option<&str>,
    limit: i64,
) -> Result<Vec<ChatMessage>, AppError> {
    let pattern = format!("%{}%", query);
    let rows = if let Some(nick) = nick {
        sqlx::query_as::<_, ChatMessage>(
            "SELECT id, nick, message, created_at \
             FROM chat_messages WHERE message ILIKE $1 AND nick = $2 \
             ORDER BY created_at DESC LIMIT $3",
        )
        .bind(&pattern)
        .bind(nick)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, ChatMessage>(
            "SELECT id, nick, message, created_at \
             FROM chat_messages WHERE message ILIKE $1 \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };
    Ok(rows)
}

pub async fn first_message(pool: &PgPool, nick: &str) -> Result<Option<ChatMessage>, AppError> {
    let row = sqlx::query_as::<_, ChatMessage>(
        "SELECT id, nick, message, created_at \
         FROM chat_messages WHERE nick = $1 ORDER BY created_at ASC LIMIT 1",
    )
    .bind(nick)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn last_message(pool: &PgPool, nick: &str) -> Result<Option<ChatMessage>, AppError> {
    let row = sqlx::query_as::<_, ChatMessage>(
        "SELECT id, nick, message, created_at \
         FROM chat_messages WHERE nick = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(nick)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// Bans
// ---------------------------------------------------------------------------

pub async fn create_ban(
    pool: &PgPool,
    nick: Option<&str>,
    ip: Option<&str>,
    reason: &str,
    banned_by: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<i32, AppError> {
    let id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO bans (nick, ip, reason, banned_by, expires_at) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(nick)
    .bind(ip)
    .bind(reason)
    .bind(banned_by)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn check_ban(pool: &PgPool, nick: &str) -> Result<Option<BanRecord>, AppError> {
    let ban = sqlx::query_as::<_, BanRecord>(
        "SELECT id, nick, ip, reason, banned_by, created_at, expires_at \
         FROM bans WHERE nick = $1 AND (expires_at IS NULL OR expires_at > NOW()) \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(nick)
    .fetch_optional(pool)
    .await?;
    Ok(ban)
}

pub async fn list_bans(pool: &PgPool) -> Result<Vec<BanRecord>, AppError> {
    let rows = sqlx::query_as::<_, BanRecord>(
        "SELECT id, nick, ip, reason, banned_by, created_at, expires_at \
         FROM bans WHERE (expires_at IS NULL OR expires_at > NOW()) \
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn delete_ban(pool: &PgPool, id: i32) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM bans WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Tells
// ---------------------------------------------------------------------------

pub async fn create_tell(
    pool: &PgPool,
    from_nick: &str,
    to_nick: &str,
    message: &str,
) -> Result<i32, AppError> {
    let id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO tells (from_nick, to_nick, message) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(from_nick)
    .bind(to_nick)
    .bind(message)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn get_pending_tells(pool: &PgPool, nick: &str) -> Result<Vec<TellRecord>, AppError> {
    let tells = sqlx::query_as::<_, TellRecord>(
        "SELECT id, from_nick, to_nick, message, created_at, delivered_at \
         FROM tells WHERE to_nick = $1 AND delivered_at IS NULL ORDER BY created_at ASC",
    )
    .bind(nick)
    .fetch_all(pool)
    .await?;
    Ok(tells)
}

pub async fn mark_tell_delivered(pool: &PgPool, id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE tells SET delivered_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Watches
// ---------------------------------------------------------------------------

pub async fn create_watch(pool: &PgPool, watcher: &str, watched: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO watches (watcher_nick, watched_nick) VALUES ($1, $2) \
         ON CONFLICT (watcher_nick, watched_nick) DO NOTHING",
    )
    .bind(watcher)
    .bind(watched)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_watchers(pool: &PgPool, nick: &str) -> Result<Vec<WatchRecord>, AppError> {
    let rows = sqlx::query_as::<_, WatchRecord>(
        "SELECT id, watcher_nick, watched_nick, created_at \
         FROM watches WHERE watched_nick = $1",
    )
    .bind(nick)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn delete_watch(pool: &PgPool, watcher: &str, watched: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM watches WHERE watcher_nick = $1 AND watched_nick = $2")
        .bind(watcher)
        .bind(watched)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

pub async fn insert_stats_snapshot(
    pool: &PgPool,
    user_count: i32,
    total_share: i64,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO stats_snapshots (user_count, total_share) VALUES ($1, $2)")
        .bind(user_count)
        .bind(total_share)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_stats_history(pool: &PgPool, limit: i64) -> Result<Vec<StatsSnapshot>, AppError> {
    let rows = sqlx::query_as::<_, StatsSnapshot>(
        "SELECT id, user_count, total_share, created_at \
         FROM stats_snapshots ORDER BY created_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Gags
// ---------------------------------------------------------------------------

pub async fn create_gag(
    pool: &PgPool,
    nick: &str,
    reason: &str,
    gagged_by: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<i32, AppError> {
    let id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO gags (nick, reason, gagged_by, expires_at) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(nick)
    .bind(reason)
    .bind(gagged_by)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn check_gag(pool: &PgPool, nick: &str) -> Result<Option<GagRecord>, AppError> {
    let gag = sqlx::query_as::<_, GagRecord>(
        "SELECT id, nick, reason, gagged_by, created_at, expires_at \
         FROM gags WHERE nick = $1 AND (expires_at IS NULL OR expires_at > NOW()) \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(nick)
    .fetch_optional(pool)
    .await?;
    Ok(gag)
}

pub async fn list_gags(pool: &PgPool) -> Result<Vec<GagRecord>, AppError> {
    let rows = sqlx::query_as::<_, GagRecord>(
        "SELECT id, nick, reason, gagged_by, created_at, expires_at \
         FROM gags WHERE (expires_at IS NULL OR expires_at > NOW()) \
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn delete_gag(pool: &PgPool, id: i32) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM gags WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Bot data (key-value storage)
// ---------------------------------------------------------------------------

pub async fn get_bot_data(
    pool: &PgPool,
    namespace: &str,
    key: &str,
) -> Result<Option<BotDataEntry>, AppError> {
    let entry = sqlx::query_as::<_, BotDataEntry>(
        "SELECT namespace, key, value, updated_at FROM bot_data WHERE namespace = $1 AND key = $2",
    )
    .bind(namespace)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(entry)
}

pub async fn list_bot_data(pool: &PgPool, namespace: &str) -> Result<Vec<BotDataEntry>, AppError> {
    let entries = sqlx::query_as::<_, BotDataEntry>(
        "SELECT namespace, key, value, updated_at FROM bot_data WHERE namespace = $1 ORDER BY key",
    )
    .bind(namespace)
    .fetch_all(pool)
    .await?;
    Ok(entries)
}

pub async fn set_bot_data(
    pool: &PgPool,
    namespace: &str,
    key: &str,
    value: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO bot_data (namespace, key, value, updated_at) VALUES ($1, $2, $3, NOW()) \
         ON CONFLICT (namespace, key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()",
    )
    .bind(namespace)
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_bot_data(pool: &PgPool, namespace: &str, key: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM bot_data WHERE namespace = $1 AND key = $2")
        .bind(namespace)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub async fn get_setting(pool: &PgPool, key: &str) -> Result<Option<String>, AppError> {
    let value = sqlx::query_scalar::<_, String>(
        "SELECT value FROM settings WHERE key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(value)
}

pub async fn set_setting(pool: &PgPool, key: &str, value: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ($1, $2, NOW()) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Maintenance
// ---------------------------------------------------------------------------

pub async fn purge_expired_bans(pool: &PgPool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "DELETE FROM bans WHERE expires_at IS NOT NULL AND expires_at <= NOW()",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn purge_expired_gags(pool: &PgPool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "DELETE FROM gags WHERE expires_at IS NOT NULL AND expires_at <= NOW()",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn close_orphaned_sessions(
    pool: &PgPool,
    online_nicks: &[String],
) -> Result<u64, AppError> {
    // Close sessions for users who are no longer in the online list
    // but still have an open session (disconnected_at IS NULL)
    let result = sqlx::query(
        "UPDATE user_sessions SET disconnected_at = NOW() \
         WHERE disconnected_at IS NULL AND user_id IN (\
           SELECT id FROM users WHERE nick != ALL($1)\
         )",
    )
    .bind(online_nicks)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
