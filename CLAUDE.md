# odch-gateway

Rust REST/WebSocket API gateway for OpenDCHub.

## Repo

- **GitHub**: github.com/typhonius/odch-gateway
- **Main branch**: `main`
- **Remote**: HTTPS

## Building and testing

```bash
cargo build
cargo test     # 77+ tests
cargo clippy
```

## Architecture

```
src/
  main.rs          — Entrypoint, tokio runtime, signal handling
  config.rs        — TOML config structs (ServerConfig, HubConfig, DatabaseConfig, etc.)
  state.rs         — AppState, HubState, HubUser
  error.rs         — AppError enum (thiserror)
  event.rs         — HubEvent enum
  bus.rs           — EventBus (broadcast channel)
  api/
    mod.rs         — Router builder (all route definitions here)
    auth.rs        — API key middleware (X-API-Key header)
    chat.rs        — GET /api/chat/history, POST /api/chat/message
    commands.rs    — GET /api/commands, POST /api/commands/:name/execute
    hub.rs         — GET /api/hub/info, GET /api/hub/stats
    moderation.rs  — kick/ban/gag/ungag via admin port
    rate_limit.rs  — Per-key rate limiting on write endpoints
    users.rs       — GET /api/users, GET /api/users/:nick, GET /api/users/:nick/history
    webhooks.rs    — CRUD for webhooks
    websocket.rs   — WebSocket with filtered event streaming
  db/
    pool.rs        — DbPool wrapper around sqlx::AnyPool (SQLite + Postgres)
    models.rs      — sqlx::FromRow structs (UserRecord, ChatHistoryEntry, WatchdogEntry)
    queries.rs     — Async query functions, table_exists() for both backends
  nmdc/
    client.rs      — NMDC TCP client with reconnect, lock-to-key auth
    admin.rs       — Admin port client (auth, commands, event stream)
    protocol.rs    — NMDC message parser
    lock_to_key.rs — Lock-to-key algorithm
  webhook/
    manager.rs     — JSON file storage for webhook configs
    delivery.rs    — HMAC-SHA256 signing, retry delivery
```

## Important notes

- **axum 0.7 uses `:param` colon syntax** for path parameters, NOT `{param}` braces (that's axum 0.8+). All routes in `api/mod.rs` must use `:nick`, `:name`, `:id`.
- **Version**: `env!("CARGO_PKG_VERSION")` is used in both the health endpoint and the NMDC tag. Always bump `Cargo.toml` version BEFORE tagging a release.
- **sqlx Any driver**: Uses `$1, $2` placeholders. Use explicit `CAST(... AS BIGINT)` for Postgres columns that might be SMALLINT/INT to avoid type mismatches.
- **Database URL format**: `sqlite:///path/to/file.db?mode=ro` or `postgres://user:pass@host:port/db`

## Config

See `config.example.toml`. Key sections: `[server]`, `[hub]`, `[admin]`, `[database]`, `[auth]`, `[webhook]`, `[rate_limit]`.

## Releases

Tag `v*` triggers `.github/workflows/release.yml` which cross-compiles linux-x86_64 and linux-aarch64 musl binaries.

```bash
# 1. Bump version in Cargo.toml
# 2. Commit and push
# 3. Tag and push
git tag v0.3.4 && git push origin v0.3.4
# 4. Create release
gh release create v0.3.4 --title "v0.3.4" --generate-notes
```

## Server deployment

```bash
# Download release binary
curl -sLO https://github.com/typhonius/odch-gateway/releases/download/v0.3.3/odch-gateway-linux-x86_64

# Deploy (binary lives at /opt/opendchub/odch-gateway, NOT /usr/local/bin)
sudo systemctl stop odch-gateway
sudo cp odch-gateway-linux-x86_64 /opt/opendchub/odch-gateway
sudo systemctl start odch-gateway
```

- Service: `odch-gateway.service` (systemd, user `opendchub`)
- Config: `/opt/opendchub/config.toml`
- Binary: `/opt/opendchub/odch-gateway`

## API endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /health | No | Health check + version |
| GET | /api/hub/info | Yes | Hub name, user count, share, connected |
| GET | /api/hub/stats | Yes | Watchdog/stats snapshots |
| GET | /api/users | Yes | Online users (live + DB enrichment) |
| GET | /api/users/:nick | Yes | Single user detail |
| GET | /api/users/:nick/history | Yes | User's chat history |
| GET | /api/chat/history | Yes | Global chat history |
| GET | /api/commands | Yes | Bot command registry |
| GET | /api/webhooks | Yes | List webhooks |
| POST | /api/chat/message | Yes | Send chat (optional `nick` for spoofing via admin) |
| POST | /api/commands/:name/execute | Yes | Execute bot command |
| POST | /api/users/:nick/kick | Yes | Kick user (admin port) |
| POST | /api/users/:nick/ban | Yes | Ban user (admin port) |
| DELETE | /api/users/:nick/ban | Yes | Unban user |
| POST | /api/users/:nick/gag | Yes | Gag user |
| DELETE | /api/users/:nick/gag | Yes | Ungag user |
| POST | /api/webhooks | Yes | Create webhook |
| PUT | /api/webhooks/:id | Yes | Update webhook |
| DELETE | /api/webhooks/:id | Yes | Delete webhook |
| GET | /ws | Query param | WebSocket event stream |
