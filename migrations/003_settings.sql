-- Key-value settings table for gateway-owned state that must survive restarts.
-- Used for: hub topic, and any future persistent configuration.

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
