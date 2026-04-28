-- Move webhooks from JSON file to database
CREATE TABLE IF NOT EXISTS webhooks (
    id UUID PRIMARY KEY,
    url TEXT NOT NULL,
    secret TEXT DEFAULT '',
    events TEXT DEFAULT '[]',
    enabled BOOLEAN DEFAULT TRUE,
    description TEXT DEFAULT '',
    created_at TIMESTAMPTZ DEFAULT NOW()
);
