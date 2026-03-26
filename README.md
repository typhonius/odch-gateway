# odch-gateway

REST and WebSocket API gateway for [OpenDCHub](https://github.com/typhonius/opendchub). Connects to a running hub via NMDC protocol and admin port, exposing a modern HTTP API for building web dashboards, bots, and integrations.

## Features

- **REST API** — Hub info, online users, chat history, moderation (kick/ban/gag), bot commands
- **WebSocket** — Real-time event stream with per-client filtering
- **Webhooks** — Outbound HTTP notifications with HMAC-SHA256 signing and retry logic
- **SQLite** — Read-only access to ODCHBot's database for historical data
- **Security** — API key auth, CORS, rate limiting, NMDC injection protection, SSRF prevention

## Installation

### Pre-built binaries (recommended)

Download the latest release for your platform from [GitHub Releases](https://github.com/typhonius/odch-gateway/releases):

```bash
# Linux x86_64
curl -LO https://github.com/typhonius/odch-gateway/releases/latest/download/odch-gateway-linux-x86_64
chmod +x odch-gateway-linux-x86_64
sudo mv odch-gateway-linux-x86_64 /usr/local/bin/odch-gateway

# Linux aarch64
curl -LO https://github.com/typhonius/odch-gateway/releases/latest/download/odch-gateway-linux-aarch64
chmod +x odch-gateway-linux-aarch64
sudo mv odch-gateway-linux-aarch64 /usr/local/bin/odch-gateway
```

### Docker

```bash
docker pull ghcr.io/typhonius/odch-gateway:latest

docker run -d \
  -p 3000:3000 \
  -v /path/to/config.toml:/etc/odch-gateway/config.toml:ro \
  -v /path/to/odchbot.db:/data/odchbot.db:ro \
  ghcr.io/typhonius/odch-gateway:latest
```

### From source

```bash
cargo install --git https://github.com/typhonius/odch-gateway.git
```

Or clone and build:

```bash
git clone https://github.com/typhonius/odch-gateway.git
cd odch-gateway
cargo build --release
# Binary at target/release/odch-gateway
```

## Quick start

1. Copy and edit the config file:

```bash
cp config.example.toml config.toml
# Edit config.toml with your hub address, admin port, API key, etc.
```

2. Run:

```bash
RUST_LOG=info odch-gateway
# or specify config path:
RUST_LOG=info odch-gateway --config /path/to/config.toml
```

3. Test the connection:

```bash
curl -H "X-API-Key: your-secret-key" http://localhost:3000/api/hub/info
```

## Configuration

See [`config.example.toml`](config.example.toml) for all options. Key sections:

| Section | Description |
|---------|-------------|
| `[server]` | Bind address, CORS origins |
| `[hub]` | Hub host/port, bot nickname, NMDC credentials |
| `[admin]` | Admin port connection for moderation commands |
| `[database]` | Path to ODCHBot's SQLite database |
| `[auth]` | API keys for authentication |
| `[webhook]` | Webhook delivery settings |
| `[rate_limit]` | Write endpoint rate limiting |

## API endpoints

All endpoints (except `/health`) require an `X-API-Key` header.

### Hub

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/hub/info` | Hub name, user count, share, uptime |
| GET | `/api/hub/stats` | Historical statistics from watchdog table |
| GET | `/health` | Health check (no auth required) |

### Users

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/users` | Online users with DB enrichment |
| GET | `/api/users/:nick` | Single user detail |
| GET | `/api/users/:nick/history` | User's chat history |

### Chat

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/chat/history` | Paginated chat history |
| POST | `/api/chat/message` | Send public chat message |

### Moderation

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/users/:nick/kick` | Kick user |
| POST | `/api/users/:nick/ban` | Ban user |
| DELETE | `/api/users/:nick/ban` | Unban user |
| POST | `/api/users/:nick/gag` | Gag (mute) user |
| DELETE | `/api/users/:nick/gag` | Ungag user |

### Bot commands

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/commands` | List registered bot commands |
| POST | `/api/commands/:name/execute` | Execute a bot command |

### Webhooks

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/webhooks` | List registered webhooks |
| POST | `/api/webhooks` | Create a webhook |
| PUT | `/api/webhooks/:id` | Update a webhook |
| DELETE | `/api/webhooks/:id` | Delete a webhook |

## WebSocket

Connect to `ws://localhost:3000/ws?api_key=YOUR_KEY&filter=chat,user_join,user_quit` to receive real-time events.

Supported event filters: `chat`, `user_join`, `user_quit`, `user_info`, `hub_name`, `op_list`, `kick`, `gateway_status`

If no filter is specified, all events are forwarded.

Example event:
```json
{"type": "Chat", "data": {"nick": "Alice", "message": "hello", "timestamp": "2026-03-25T12:00:00Z"}}
```

## Webhooks

Register a webhook to receive HTTP POST notifications for hub events:

```bash
curl -X POST http://localhost:3000/api/webhooks \
  -H "X-API-Key: your-key" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com/hook", "secret": "my-hmac-secret", "events": ["chat", "user_join"]}'
```

Payloads are signed with HMAC-SHA256 in the `X-Webhook-Signature` header. Failed deliveries retry up to 3 times.

## Architecture

```
Web App ──> odch-gateway (Rust) ──> OpenDCHub
              ├── REST API :3000       ├── NMDC port (chat, events)
              ├── WebSocket /ws        ├── Admin port (moderation)
              └── Webhooks (outbound)  └── ODCHBot SQLite DB
```

The gateway maintains two connections to the hub:
- **NMDC client** — Joins as a regular user to observe chat, joins/quits, and send messages
- **Admin client** — Connects to the admin port for moderation commands and the event stream

Both connections auto-reconnect with exponential backoff.

## Development

```bash
# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run

# Check for issues
cargo clippy
```

## License

MIT
