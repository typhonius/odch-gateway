# odch-gateway

REST/WebSocket API gateway and bot platform for [OpenDCHub](https://github.com/typhonius/opendchub). Owns the PostgreSQL database, provides built-in moderation commands, and lets external bots register as virtual hub users over pure HTTP.

## Features

- Connects to hub via Unix domain socket (JSON protocol)
- Owns all persistent data in PostgreSQL (chat, users, bans, tells, stats, webhooks)
- 18 built-in moderation commands (ban, tell, history, stats, kick, gag, etc.)
- **Bot platform** — external bots register via API, appear as virtual users on the hub
- **SSE event streaming** — bots receive commands and PMs in real-time
- WebSocket event streaming for web clients
- Admin web UI (Preact + Pico CSS, embedded in binary)
- Webhook delivery with HMAC-SHA256 signing
- `odch-gateway init` for one-command setup

## Quick Start

```bash
# Download binary from GitHub releases
curl -sLO https://github.com/typhonius/odch-gateway/releases/latest/download/odch-gateway-linux-x86_64
chmod +x odch-gateway-linux-x86_64

# Set up everything (creates DB, generates secrets, writes configs)
./odch-gateway-linux-x86_64 init --db-password YOUR_PG_PASSWORD

# Start
systemctl start opendchub odch-gateway
```

## Architecture

```
┌──────────┐  HTTP + SSE  ┌──────────────┐  Unix socket  ┌───────────┐
│  bot     │─────────────→│              │──────────────→│           │
│ (any     │  register    │   gateway    │  virtual      │ opendchub │←→ DC Clients
│  lang)   │  SSE/chat/pm │              │  users        │  (NMDC)   │
└──────────┘              │              │               └───────────┘
                          │  PostgreSQL  │
                          │  Admin UI    │
                          │  Webhooks    │
                          └──────────────┘
```

The gateway is fully generic — zero bot-specific code. Bots register via the API which creates virtual users on the hub. Hub + gateway is a complete system without any bots; core features (bans, tells, chat history, stats) are built in. See [ODCHBot](https://github.com/typhonius/odchbot) for a reference bot implementation.

## Writing a Bot

Any language that speaks HTTP can be a bot. Here's the full lifecycle:

### 1. Register

```bash
curl -X POST http://localhost:3000/api/v1/bot/register \
  -H "X-API-Key: YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "nick": "MyBot",
    "description": "A custom bot",
    "email": "bot@example.com",
    "tag": "<mybot V:1.0.0>",
    "commands": ["hello", "dice"]
  }'
```

This creates a virtual user on the hub — "MyBot" appears in the DC user list without needing an NMDC connection.

### 2. Receive events (SSE)

```bash
curl -N -H "X-API-Key: YOUR_KEY" \
  "http://localhost:3000/api/v1/bot/events?nick=MyBot"
```

The gateway streams Server-Sent Events:

```
event: command
data: {"event_type":"Command","from_nick":"Adam","command":"hello","args":"world","timestamp":"..."}

event: pm
data: {"event_type":"PrivateMessage","from_nick":"Adam","message":"hey bot","timestamp":"..."}
```

`command` events fire when someone types `!hello` or `!dice` in chat. `pm` events fire when someone sends a private message to the bot.

### 3. Respond

```bash
# Public chat (as the bot)
curl -X POST http://localhost:3000/api/v1/bot/chat \
  -H "X-API-Key: YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{"nick": "MyBot", "message": "Hello world!"}'

# Private message (as the bot)
curl -X POST http://localhost:3000/api/v1/bot/pm \
  -H "X-API-Key: YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{"from": "MyBot", "to": "Adam", "message": "Hey!"}'
```

### 4. Unregister

```bash
curl -X DELETE http://localhost:3000/api/v1/bot/register \
  -H "X-API-Key: YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{"nick": "MyBot"}'
```

The virtual user disappears from the hub. Bots typically send this on shutdown.

## Building

```bash
cargo build --release
cargo test
cargo clippy -- -D warnings
```

## Configuration

See `config.example.toml`. Key sections:

```toml
[hub]
socket_path = "/opt/opendchub/gateway.sock"
secret = "shared_secret"

[database]
url = "postgres://odch:password@localhost:5432/odch"

[auth]
api_keys = ["your_api_key"]

[admin_ui]
bind_address = "127.0.0.1:3001"
```

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | /health | Health check |
| GET | /api/v1/hub/info | Hub name, users, share, uptime |
| GET | /api/v1/users | Online users |
| GET | /api/v1/chat/history | Chat history |
| POST | /api/v1/chat/message | Send chat message |
| POST | /api/v1/users/:nick/kick | Kick user |
| POST | /api/v1/users/:nick/ban | Ban user |
| GET | /ws | WebSocket event stream |

### Bot Platform

| Method | Path | Description |
|--------|------|-------------|
| POST | /api/v1/bot/register | Register bot (creates virtual hub user) |
| DELETE | /api/v1/bot/register | Unregister bot (removes virtual user) |
| GET | /api/v1/bot/events?nick=X | SSE stream (commands + PMs) |
| POST | /api/v1/bot/chat | Send public chat as bot |
| POST | /api/v1/bot/pm | Send PM as bot |
| GET | /api/v1/bot/commands/pending?nick=X | Poll for commands (alternative to SSE) |

Full data API under `/api/v1/bot/` for tells, bans, quotes, watches, stats, and key-value storage.

## Related

- [opendchub](https://github.com/typhonius/opendchub) — NMDC hub server
- [odchbot](https://github.com/typhonius/odchbot) — Reference bot (Dragon + OPChat)
