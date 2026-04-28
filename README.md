# odch-gateway

REST/WebSocket API gateway for [OpenDCHub](https://github.com/typhonius/opendchub). Owns the PostgreSQL database. Provides built-in bot commands, a bot API, webhooks, and an admin web UI.

## Features

- Connects to hub via Unix domain socket (JSON protocol)
- Owns all persistent data in PostgreSQL (chat, users, bans, tells, stats, webhooks)
- 18 built-in bot commands (ban, tell, history, stats, kick, gag, etc.)
- Bot API for external bots like [Dragon](https://github.com/typhonius/odchbot)
- WebSocket event streaming
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
DC Clients ──→ opendchub ←──→ odch-gateway ──→ PostgreSQL
               (NMDC)         (Unix socket)
                                  │
                          REST API + WebSocket
                          Admin UI + Webhooks
                          Built-in bot commands
```

Hub + Gateway is a fully functional system. No bot needed for core features (bans, tells, chat history, stats). [ODCHBot](https://github.com/typhonius/odchbot) is an optional add-on for fun commands and plugins.

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

Full bot API under `/api/v1/bot/` for tells, bans, quotes, watches, stats, and key-value storage.

## Related

- [opendchub](https://github.com/typhonius/opendchub) — NMDC hub server
- [odchbot](https://github.com/typhonius/odchbot) — Optional standalone bot
