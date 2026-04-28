# odch-gateway

Rust REST/WebSocket API gateway for OpenDCHub. Owns the PostgreSQL database. Provides built-in bot commands, a bot API for Dragon, and an admin web UI.

## Building and testing

```bash
cargo build
cargo test
cargo clippy -- -D warnings
```

Requires PostgreSQL for integration tests. Set `DATABASE_URL` or `TEST_DATABASE_URL`.

## Architecture

```
src/
  main.rs          — Entry point, CLI (server + init subcommand)
  init.rs          — `odch-gateway init` setup wizard
  config.rs        — TOML config structs
  state.rs         — AppState, HubState, HubUser
  error.rs         — AppError enum
  event.rs         — HubEvent enum
  bus.rs           — EventBus (broadcast channel)
  hub/
    mod.rs         — Module declaration
    socket.rs      — Unix socket client (JSON protocol to hub)
  bot/
    mod.rs         — CommandEngine (built-in bot commands)
    commands/
      mod.rs       — 18 commands: ban, tell, history, stats, kick, etc.
  api/
    mod.rs         — Router (all route definitions)
    auth.rs        — API key middleware
    bot.rs         — Bot API endpoints (/api/v1/bot/*)
    chat.rs        — Chat history + send
    commands.rs    — List/execute bot commands
    hub.rs         — Hub info + stats
    moderation.rs  — Kick/ban/gag via hub
    rate_limit.rs  — Per-key rate limiting
    users.rs       — User list + detail
    webhooks.rs    — Webhook CRUD
    websocket.rs   — WebSocket event streaming
  db/
    pool.rs        — PgPool wrapper, runs migrations on startup
    models.rs      — sqlx::FromRow structs
    queries.rs     — All SQL queries
    event_processor.rs — Stores events, delivers tells, notifies watchers
  admin_ui/
    mod.rs         — Admin UI router + static asset serving (rust-embed)
    auth.rs        — JWT session auth (login/logout)
  webhook/
    manager.rs     — PostgreSQL webhook CRUD
    delivery.rs    — HMAC-SHA256 signing, retry delivery

admin-ui/          — Preact + HTM + Pico CSS (embedded in binary)
migrations/        — sqlx PostgreSQL migrations
```

## Config

See `config.example.toml`. Sections: `[server]`, `[hub]`, `[database]`, `[auth]`, `[webhook]`, `[rate_limit]`, `[admin_ui]`.

Hub connection is via Unix domain socket (`[hub].socket_path`). PostgreSQL required (`[database].url`).

## Releases

Tag `v*` triggers `.github/workflows/release.yml` which cross-compiles linux-x86_64 and linux-aarch64 musl binaries.

## Server

- Binary: `/opt/opendchub/odch-gateway`
- Config: `/opt/opendchub/config.toml`
- Service: `odch-gateway.service`
- Database: PostgreSQL `odch`
