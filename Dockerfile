FROM --platform=linux/amd64 rust:1.89-alpine3.22 AS builder
# Install build dependencies inklusive statischer OpenSSL-Bibliotheken
RUN apk update && apk add --no-cache \
    musl-dev gcc g++ make perl pkgconfig \
    openssl-dev openssl-libs-static
WORKDIR /app
# Setze Umgebungsvariablen für statisches OpenSSL
ENV OPENSSL_STATIC=yes
ENV OPENSSL_VENDORED=yes
# Kopiere Cargo.toml und src für einen effizienten Build-Cache
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# Baue das Release-Binary für linux/amd64
RUN cargo build --release

FROM --platform=linux/amd64 alpine:3.22.1
# Installiere minimale Laufzeitabhängigkeiten (z.B. OpenSSL, falls dynamisch gelinkt)
RUN apk add --no-cache libgcc libssl3
WORKDIR /app
# Kopiere das gebaute Binary aus dem Builder-Image
COPY --from=builder /app/target/release/synology-telegram-bot /app/synology-telegram-bot

# Dokumentation der Umgebungsvariablen
# STB_TELEGRAM_BOT_TOKEN - Erforderlich: Dein Telegram-Bot-Token
# STB_SYNOLOGY_NAS_BASE_URL - Erforderlich: Basis-URL deines Synology NAS
# STB_SYNOLOGY_PASSWORD - Erforderlich: NAS-Passwort
# STB_ALLOWED_CHAT_ID - Erforderlich: Erlaubte Telegram Chat-ID
# STB_FORCE_IPV4 - Optional: "true" oder "1" für IPv4 (Standard: false)
# STB_RUST_LOG - Optional: Log-Level (Standard: info)

ENTRYPOINT ["/app/synology-telegram-bot"]
