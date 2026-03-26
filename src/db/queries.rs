use crate::db::models::{ChatHistoryEntry, UserRecord, WatchdogEntry};
use crate::db::pool::DbPool;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// Public query functions
// ---------------------------------------------------------------------------

/// Look up a single user by nickname.
pub async fn get_user(pool: &DbPool, nick: &str) -> Result<Option<UserRecord>, AppError> {
    let user = sqlx::query_as::<_, UserRecord>(
        "SELECT name, ip, share, description, email, speed, \
                connect_time, disconnect_time, permission \
         FROM users WHERE name = $1",
    )
    .bind(nick)
    .fetch_optional(pool.inner())
    .await?;

    Ok(user)
}

/// List users ordered by most-recent login, with pagination.
pub async fn list_users(
    pool: &DbPool,
    limit: i64,
    offset: i64,
) -> Result<Vec<UserRecord>, AppError> {
    let users = sqlx::query_as::<_, UserRecord>(
        "SELECT name, ip, share, description, email, speed, \
                connect_time, disconnect_time, permission \
         FROM users ORDER BY connect_time DESC LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool.inner())
    .await?;

    Ok(users)
}

/// Return the most recent chat history lines (newest first), with pagination.
///
/// The history table stores `uid` references, so we JOIN to get the nick.
pub async fn get_chat_history(
    pool: &DbPool,
    limit: i64,
    offset: i64,
) -> Result<Vec<ChatHistoryEntry>, AppError> {
    let history = sqlx::query_as::<_, ChatHistoryEntry>(
        "SELECT h.hid, u.name AS nickname, h.chat, h.time \
         FROM history h \
         JOIN users u ON u.uid = h.uid \
         ORDER BY h.hid DESC \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool.inner())
    .await?;

    Ok(history)
}

/// Return chat history for a specific user (newest first), with pagination.
pub async fn get_user_chat_history(
    pool: &DbPool,
    nick: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<ChatHistoryEntry>, AppError> {
    let history = sqlx::query_as::<_, ChatHistoryEntry>(
        "SELECT h.hid, u.name AS nickname, h.chat, h.time \
         FROM history h \
         JOIN users u ON u.uid = h.uid \
         WHERE u.name = $1 \
         ORDER BY h.hid DESC \
         LIMIT $2 OFFSET $3",
    )
    .bind(nick)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool.inner())
    .await?;

    Ok(history)
}

/// Return recent hub snapshots from the stats table.
///
/// Handles both v3 (`watchdog` table with `wid, users, share`) and
/// v4 (`stats` table with `sid, number_users, total_share`) schemas by
/// probing for the table that exists and aliasing columns.
pub async fn get_hub_stats(pool: &DbPool, limit: i64) -> Result<Vec<WatchdogEntry>, AppError> {
    let sql = if table_exists(pool, "stats").await {
        "SELECT sid AS id, number_users AS users_online, \
                total_share AS total_share, time \
         FROM stats ORDER BY sid DESC LIMIT $1"
    } else if table_exists(pool, "watchdog").await {
        "SELECT wid AS id, users AS users_online, \
                share AS total_share, time \
         FROM watchdog ORDER BY wid DESC LIMIT $1"
    } else {
        return Ok(Vec::new());
    };

    let stats = sqlx::query_as::<_, WatchdogEntry>(sql)
        .bind(limit)
        .fetch_all(pool.inner())
        .await?;

    Ok(stats)
}

/// Helper: check whether a table exists in the database.
///
/// Uses `sqlite_master` for SQLite and `information_schema.tables` for Postgres.
pub async fn table_exists(pool: &DbPool, name: &str) -> bool {
    let sql = if pool.is_postgres() {
        "SELECT 1 FROM information_schema.tables WHERE table_name = $1"
    } else {
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name = $1"
    };

    sqlx::query(sql)
        .bind(name)
        .fetch_optional(pool.inner())
        .await
        .ok()
        .flatten()
        .is_some()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_test_pool;

    /// Execute multiple SQL statements separated by semicolons.
    async fn execute_batch(pool: &DbPool, sql: &str) {
        for statement in sql.split(';') {
            let trimmed = statement.trim();
            // Skip empty chunks and comment-only chunks
            let has_sql = trimmed
                .lines()
                .any(|l| !l.trim().is_empty() && !l.trim().starts_with("--"));
            if has_sql {
                sqlx::query(trimmed)
                    .execute(pool.inner())
                    .await
                    .unwrap_or_else(|e| panic!("batch exec failed on: {trimmed}\nerror: {e}"));
            }
        }
    }

    /// Set up the in-memory database with the v4 schema and some seed data.
    async fn seed_db(pool: &DbPool) {
        execute_batch(
            pool,
            "CREATE TABLE users (
                uid             INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT    NOT NULL,
                ip              TEXT    DEFAULT '',
                email           TEXT    DEFAULT '',
                share           INTEGER DEFAULT 0,
                share_delta     INTEGER DEFAULT 0,
                permission      INTEGER DEFAULT 4,
                connect_time    INTEGER,
                disconnect_time INTEGER,
                description     TEXT    DEFAULT '',
                speed           TEXT    DEFAULT ''
            );

            CREATE TABLE history (
                hid  INTEGER PRIMARY KEY AUTOINCREMENT,
                time INTEGER NOT NULL,
                uid  INTEGER NOT NULL,
                chat TEXT    NOT NULL
            );

            CREATE TABLE stats (
                sid            INTEGER PRIMARY KEY AUTOINCREMENT,
                time           INTEGER NOT NULL,
                number_users   INTEGER DEFAULT 0,
                total_share    INTEGER DEFAULT 0,
                connections    INTEGER DEFAULT 0,
                disconnections INTEGER DEFAULT 0
            );

            -- Seed users
            INSERT INTO users (name, ip, share, description, email, speed, connect_time, disconnect_time, permission)
                VALUES ('alice', '10.0.0.1', 1024, 'Alice desc', 'alice@example.com', 'LAN(T1)', 1700000000, NULL, 8);
            INSERT INTO users (name, ip, share, description, email, speed, connect_time, disconnect_time, permission)
                VALUES ('bob', '10.0.0.2', 2048, 'Bob desc', 'bob@example.com', '56Kbps', 1700000100, 1700000200, 4);
            INSERT INTO users (name, ip, share, description, email, speed, connect_time, disconnect_time, permission)
                VALUES ('charlie', '10.0.0.3', 4096, 'Charlie desc', '', 'Cable', 1700000050, NULL, 16);

            -- Seed history (uid references users.uid)
            INSERT INTO history (time, uid, chat) VALUES (1700000010, 1, 'Hello everyone!');
            INSERT INTO history (time, uid, chat) VALUES (1700000020, 2, 'Hi alice!');
            INSERT INTO history (time, uid, chat) VALUES (1700000030, 1, 'How are you?');
            INSERT INTO history (time, uid, chat) VALUES (1700000040, 3, 'I am the operator.');

            -- Seed stats
            INSERT INTO stats (time, number_users, total_share) VALUES (1700000000, 3, 7168);
            INSERT INTO stats (time, number_users, total_share) VALUES (1700000060, 2, 3072);
            INSERT INTO stats (time, number_users, total_share) VALUES (1700000120, 3, 7168)",
        )
        .await;
    }

    #[tokio::test]
    async fn test_get_user_found() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let user = get_user(&pool, "alice").await.unwrap();
        assert!(user.is_some());
        let user = user.unwrap();
        assert_eq!(user.nick, "alice");
        assert_eq!(user.ip, "10.0.0.1");
        assert_eq!(user.share, 1024);
        assert_eq!(user.description, "Alice desc");
        assert_eq!(user.email, "alice@example.com");
        assert_eq!(user.speed, "LAN(T1)");
        assert_eq!(user.connect_time, Some(1700000000));
        assert!(user.disconnect_time.is_none());
        assert_eq!(user.permissions, 8);
    }

    #[tokio::test]
    async fn test_get_user_not_found() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let user = get_user(&pool, "nonexistent").await.unwrap();
        assert!(user.is_none());
    }

    #[tokio::test]
    async fn test_list_users_all() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let users = list_users(&pool, 100, 0).await.unwrap();
        assert_eq!(users.len(), 3);
        // Ordered by connect_time DESC: bob(1700000100), charlie(1700000050), alice(1700000000)
        assert_eq!(users[0].nick, "bob");
        assert_eq!(users[1].nick, "charlie");
        assert_eq!(users[2].nick, "alice");
    }

    #[tokio::test]
    async fn test_list_users_pagination() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let page1 = list_users(&pool, 2, 0).await.unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].nick, "bob");
        assert_eq!(page1[1].nick, "charlie");

        let page2 = list_users(&pool, 2, 2).await.unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].nick, "alice");
    }

    #[tokio::test]
    async fn test_get_chat_history() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let history = get_chat_history(&pool, 10, 0).await.unwrap();
        assert_eq!(history.len(), 4);
        // Ordered by hid DESC (newest first)
        assert_eq!(history[0].nickname, "charlie");
        assert_eq!(history[0].chat, "I am the operator.");
        assert_eq!(history[1].nickname, "alice");
        assert_eq!(history[1].chat, "How are you?");
        assert_eq!(history[3].nickname, "alice");
        assert_eq!(history[3].chat, "Hello everyone!");
    }

    #[tokio::test]
    async fn test_get_chat_history_pagination() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let page = get_chat_history(&pool, 2, 1).await.unwrap();
        assert_eq!(page.len(), 2);
        // Skip 1 (the newest), get next 2
        assert_eq!(page[0].chat, "How are you?");
        assert_eq!(page[1].chat, "Hi alice!");
    }

    #[tokio::test]
    async fn test_get_user_chat_history() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let history = get_user_chat_history(&pool, "alice", 10, 0).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].chat, "How are you?");
        assert_eq!(history[1].chat, "Hello everyone!");
    }

    #[tokio::test]
    async fn test_get_user_chat_history_no_results() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let history = get_user_chat_history(&pool, "nonexistent", 10, 0)
            .await
            .unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_get_hub_stats_v4() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let stats = get_hub_stats(&pool, 10).await.unwrap();
        assert_eq!(stats.len(), 3);
        // Ordered by sid DESC (newest first)
        assert_eq!(stats[0].users_online, 3);
        assert_eq!(stats[0].total_share, 7168);
        assert_eq!(stats[0].timestamp, 1700000120);
        assert_eq!(stats[1].users_online, 2);
        assert_eq!(stats[1].total_share, 3072);
    }

    #[tokio::test]
    async fn test_get_hub_stats_v3_fallback() {
        let pool = create_test_pool().await;

        // Create v3 watchdog table instead of v4 stats.
        execute_batch(
            &pool,
            "CREATE TABLE watchdog (
                wid             INTEGER PRIMARY KEY AUTOINCREMENT,
                time            INTEGER,
                users           INTEGER,
                share           INTEGER,
                connections     INTEGER,
                disconnections  INTEGER,
                searches        INTEGER
            );
            INSERT INTO watchdog (time, users, share) VALUES (1700000000, 5, 9000);
            INSERT INTO watchdog (time, users, share) VALUES (1700000060, 7, 12000)",
        )
        .await;

        let stats = get_hub_stats(&pool, 10).await.unwrap();
        assert_eq!(stats.len(), 2);
        // Ordered by wid DESC
        assert_eq!(stats[0].users_online, 7);
        assert_eq!(stats[0].total_share, 12000);
        assert_eq!(stats[1].users_online, 5);
    }

    #[tokio::test]
    async fn test_get_hub_stats_no_table() {
        let pool = create_test_pool().await;
        // Empty DB, no stats or watchdog table
        let stats = get_hub_stats(&pool, 10).await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_get_hub_stats_limit() {
        let pool = create_test_pool().await;
        seed_db(&pool).await;

        let stats = get_hub_stats(&pool, 1).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].timestamp, 1700000120);
    }
}
