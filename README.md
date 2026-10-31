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
