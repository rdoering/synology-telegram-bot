FROM --platform=linux/amd64 golang:1.27-alpine AS builder
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY cmd ./cmd
COPY internal ./internal
ARG STB_VERSION=dev
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w -X main.version=${STB_VERSION}" -o /out/synology-telegram-bot ./cmd/bot

FROM --platform=linux/amd64 alpine:3.22.1
RUN apk add --no-cache ca-certificates
WORKDIR /app
COPY --from=builder /out/synology-telegram-bot /app/synology-telegram-bot

# Umgebungsvariablen (siehe README.md):
# STB_TELEGRAM_BOT_TOKEN, STB_SYNOLOGY_NAS_BASE_URL, STB_SYNOLOGY_USERNAME,
# STB_SYNOLOGY_PASSWORD, STB_ALLOWED_CHAT_ID  - erforderlich
# STB_FORCE_IPV4, STB_LOG_LEVEL               - optional
# STB_BAO_ADDR, STB_UNSEAL_WEB_URL            - optional (OpenBao)
# STB_BAO_POLL_INTERVAL                       - optional (Standard: 10s)
# STB_METRICS_ADDR                            - optional (Standard: :9118, "off" = aus)

EXPOSE 9118

ENTRYPOINT ["/app/synology-telegram-bot"]
