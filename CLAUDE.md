# odch-gateway

Rust REST/WebSocket API gateway for OpenDCHub. Owns the PostgreSQL database. Provides built-in bot commands (moderation + fun), manages virtual users (ODCHBot/OPChat) on the hub, and hosts an admin web UI.

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
  main.rs          — Entry point, CLI, spawns all tasks (event processor, greeter, bot, webhooks, maintenance timer)
  init.rs          — `odch-gateway init` setup wizard
  config.rs        — TOML config structs
  state.rs         — AppState, HubState, HubUser
  error.rs         — AppError enum
  event.rs         — HubEvent enum (hub events + gateway-originated: Ban, Gag, MaintenanceTick)
  bus.rs           — EventBus (broadcast channel)
  greeter.rs       — Connection greeter (sends topic + welcome on UserJoin)
  hub/
    mod.rs         — Module declaration
    socket.rs      — Unix socket client (JSON protocol to hub), chat mediation (gag check + echo)
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
    moderation.rs  — Kick/ban/gag (ban/gag are DB-only, kick is NMDC)
    rate_limit.rs  — Per-key rate limiting
    users.rs       — User list + detail
    webhooks.rs    — Webhook CRUD
    websocket.rs   — WebSocket event streaming
  db/
    pool.rs        — PgPool wrapper, runs migrations on startup
    models.rs      — sqlx::FromRow structs
    queries.rs     — All SQL queries
    event_processor.rs — Stores events, delivers tells, notifies watchers, ban enforcement, maintenance
  admin_ui/
    mod.rs         — Admin UI router + static asset serving (rust-embed)
    auth.rs        — JWT session auth (login/logout)
  webhook/
    manager.rs     — PostgreSQL webhook CRUD
    delivery.rs    — HMAC-SHA256 signing, retry delivery

admin-ui/          — Preact + HTM + Pico CSS (embedded in binary)
migrations/        — sqlx PostgreSQL migrations
```

## Moderation ownership

- **Kick**: NMDC protocol (hub disconnects TCP). Gateway sends `kick` command. Hub emits `HubEvent::Kick`.
- **Ban**: Gateway DB-only. Enforced on `UserJoin` (event processor checks ban list, kicks if banned). Gateway publishes `HubEvent::Ban`/`Unban`.
- **Gag**: Gateway DB-only. Enforced in chat mediation (`socket.rs` checks gag on every chat event). Gateway publishes `HubEvent::Gag`/`Ungag`.
- **Topic**: Gateway-owned. Stored in `HubState::topic`. Broadcast via `send_raw`, sent to new users by greeter.

## Event bus

Tokio broadcast channel. Events published by:
- Hub socket client (chat, user_join, user_quit, myinfo, kick, pm)
- Bot commands and moderation API (Ban, Unban, Gag, Ungag)
- Maintenance timer (MaintenanceTick every N seconds, configurable)

Subscribers: event processor, greeter, bot command processor, webhook dispatcher, WebSocket clients, event logger.

## Config

See `config.example.toml`. Sections: `[server]`, `[hub]`, `[database]`, `[auth]`, `[webhook]`, `[rate_limit]`, `[greeting]`, `[admin_ui]`.

Hub connection is via Unix domain socket (`[hub].socket_path`). PostgreSQL required (`[database].url`). `[hub].maintenance_interval_secs` controls the MaintenanceTick frequency (default: 60s).

## Releases

Tag `v*` triggers `.github/workflows/release.yml` which cross-compiles linux-x86_64 and linux-aarch64 musl binaries.

## Server

- Binary: `/opt/opendchub/odch-gateway`
- Config: `/opt/opendchub/config.toml`
- Service: `odch-gateway.service`
- Database: PostgreSQL `odch`
