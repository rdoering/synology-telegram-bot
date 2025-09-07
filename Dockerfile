FROM --platform=linux/amd64 rust:1.89-alpine3.22 AS chef
# Install build dependencies including static OpenSSL libraries
RUN apk update && apk add --no-cache \
    musl-dev gcc g++ make perl pkgconfig \
    openssl-dev openssl-libs-static
WORKDIR /app
# Set environment variables for vendored OpenSSL (native compilation only)
ENV OPENSSL_STATIC=yes
ENV OPENSSL_VENDORED=yes

# Set environment variables for vendored OpenSSL (native x86_64 compilation)
RUN cargo build --release

FROM alpine:3.22.1
# Build natively for x86_64 (no cross-compilation needed)
RUN cargo build --release

# Install runtime dependencies
FROM --platform=linux/amd64 alpine:3.22.1

# Copy the binary from the builder stage (native architecture)
COPY --from=chef /app/target/release/synology-telegram-bot /app/synology-telegram-bot

# Document required environment variables
# STB_TELEGRAM_BOT_TOKEN - Required: Your Telegram bot token
# STB_SYNOLOGY_NAS_BASE_URL - Required: Base URL of your Synology NAS (e.g., http://your-nas-ip:port)
COPY --from=chef /app/target/release/synology-telegram-bot /app/synology-telegram-bot
# STB_SYNOLOGY_PASSWORD - Required: Your Synology NAS password
# STB_ALLOWED_CHAT_ID - Required: Your Telegram chat ID that is allowed to use the bot
# STB_FORCE_IPV4 - Optional: Set to "true" or "1" to force IPv4 connections (default: false)
# STB_RUST_LOG - Optional: Set the log level (default: info)

# Set the entrypoint
ENTRYPOINT ["/app/synology-telegram-bot"]
