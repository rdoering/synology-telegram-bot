# Synology Telegram Bot

A Telegram bot for controlling the SSH service of a Synology NAS and for
unsealing/monitoring an OpenBao server. Written in Go.

## Features

- `/start` – main menu (inline keyboard), inline query hint, command menu button
- SSH control of the NAS: `/ssh [on|off]`, `/ssh_on`, `/ssh_off` or via menu
- OpenBao (optional):
  - `/seal_status` – show the current seal status
  - `/unseal` – link-based unseal: the bot sends a one-time link to the web
    app, you encrypt the unseal key there with an ephemeral age key and paste
    the ciphertext back into the chat (valid 5 minutes, single use; the
    ciphertext message is deleted afterwards)
  - **Automatic status messages:** after startup the bot posts the current
    seal status once, and again every time it changes (sealed/unsealed,
    not initialized, unreachable) – including unexpected changes such as a
    re-seal after a server restart.

Only the chat configured in `STB_ALLOWED_CHAT_ID` may use the bot.

## Configuration

Environment variables (a `.env` file is loaded if present; real environment
variables take precedence):

| Variable | Required | Description |
|---|---|---|
| `STB_TELEGRAM_BOT_TOKEN` | yes | Token from [@BotFather](https://t.me/BotFather) |
| `STB_SYNOLOGY_NAS_BASE_URL` | yes | e.g. `http://192.168.1.100:5000` |
| `STB_SYNOLOGY_USERNAME` / `STB_SYNOLOGY_PASSWORD` | yes | NAS credentials |
| `STB_ALLOWED_CHAT_ID` | yes | Telegram chat id allowed to use the bot and receiving status messages |
| `STB_FORCE_IPV4` | no | `true`/`1` to use IPv4 only |
| `STB_LOG_LEVEL` | no | `debug`, `info` (default), `warn`, `error` (`STB_RUST_LOG` still accepted) |
| `STB_BAO_ADDR` | no | OpenBao address, e.g. `http://openbao:8200` |
| `STB_UNSEAL_WEB_URL` | no | Base URL of the unseal web app. Both `STB_BAO_*` and this one are needed to enable OpenBao features |
| `STB_BAO_POLL_INTERVAL` | no | Seal status polling interval, Go duration (default `10s`) |
| `STB_METRICS_ADDR` | no | Listen address of the Prometheus `/metrics` + `/healthz` endpoint (default `:9118`, `off` disables it) |

Empty values count as unset. Passwords with special characters should be
wrapped in single quotes in `.env`.

## Run

```
cp .env.example .env   # edit
go run ./cmd/bot
```

Docker:

```
docker compose up --build
```

## Development

```
just vet     # go vet
just test    # tests with coverage
just crap    # fails if any function has a CRAP score >= 30
just build   # static binary in bin/
```

Layout: `cmd/bot` (wiring), `internal/bot` (commands, menus, Telegram adapter),
`internal/synology` (DSM client), `internal/bao` (OpenBao client + age),
`internal/unseal` (session store), `internal/watch` (seal status monitor),
`internal/config`, `tools/crap` (CRAP score checker).

To add a command, extend `Handler.command` in `internal/bot/handler.go` and
the `Commands` list in `internal/bot/telegram.go`.

## Metrics

The bot exposes Prometheus metrics on `STB_METRICS_ADDR` (default `:9118`, path
`/metrics`; `/healthz` for liveness). No authentication – publish the port only
inside the trusted network.

| Metric | Labels | Meaning |
|---|---|---|
| `stb_build_info` | `version` | build version (value 1) |
| `stb_start_time_seconds` | – | process start time |
| `stb_telegram_updates_received_total` | `type`, `authorized` | updates received from users |
| `stb_commands_total` | `cmd` | commands from the allowed chat |
| `stb_unauthorized_attempts_total` | – | requests from chats that are not allowed |
| `stb_telegram_messages_sent_total` / `stb_telegram_send_errors_total` | `kind` | Telegram API calls / failures |
| `stb_synology_requests_total` | `op`, `result` | DSM operations (`ssh_status`, `ssh_set`) |
| `stb_synology_errors_total` | `code` | DSM error code (`other` = non-DSM error) |
| `stb_synology_last_success_timestamp_seconds` | – | last successful DSM operation |
| `stb_synology_request_duration_seconds_{sum,count}` | `op` | DSM operation duration |
| `stb_bao_polls_total` | `result` | OpenBao seal status polls |
| `stb_bao_state` | `state` | current OpenBao state (one-hot) |
| `stb_unseal_attempts_total` | `result` | unseal attempts by outcome |
