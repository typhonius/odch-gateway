-- ODCHub Gateway Schema v1
-- Gateway owns all persistent state.

CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    nick VARCHAR(64) UNIQUE NOT NULL,
    email VARCHAR(255) DEFAULT '',
    password_hash VARCHAR(255),
    permission SMALLINT DEFAULT 0,
    share_size BIGINT DEFAULT 0,
    description TEXT DEFAULT '',
    speed VARCHAR(32) DEFAULT '',
    first_seen TIMESTAMPTZ DEFAULT NOW(),
    last_seen TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_sessions (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id),
    login_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    logout_at TIMESTAMPTZ,
    ip_address VARCHAR(45),
    tls BOOLEAN DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS chat_messages (
    id BIGSERIAL PRIMARY KEY,
    nick VARCHAR(64) NOT NULL,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_chat_created ON chat_messages(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_chat_nick ON chat_messages(nick);

CREATE TABLE IF NOT EXISTS bans (
    id SERIAL PRIMARY KEY,
    nick VARCHAR(64),
    ip VARCHAR(45),
    reason TEXT DEFAULT '',
    banned_by VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS tells (
    id SERIAL PRIMARY KEY,
    from_nick VARCHAR(64) NOT NULL,
    to_nick VARCHAR(64) NOT NULL,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    delivered_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_tells_pending ON tells(to_nick) WHERE delivered_at IS NULL;

CREATE TABLE IF NOT EXISTS quotes (
    id SERIAL PRIMARY KEY,
    nick VARCHAR(64) NOT NULL,
    quote_text TEXT NOT NULL,
    added_by VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS watches (
    id SERIAL PRIMARY KEY,
    watcher_nick VARCHAR(64) NOT NULL,
    watched_nick VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(watcher_nick, watched_nick)
);

CREATE TABLE IF NOT EXISTS stats_snapshots (
    id SERIAL PRIMARY KEY,
    user_count INT NOT NULL,
    total_share BIGINT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS gags (
    id SERIAL PRIMARY KEY,
    nick VARCHAR(64) NOT NULL,
    reason TEXT DEFAULT '',
    gagged_by VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS bot_commands (
    name VARCHAR(64) PRIMARY KEY,
    description TEXT DEFAULT '',
    aliases TEXT DEFAULT '[]',
    permission SMALLINT DEFAULT 0,
    enabled BOOLEAN DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS bot_data (
    namespace VARCHAR(64) NOT NULL,
    key VARCHAR(255) NOT NULL,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (namespace, key)
);
