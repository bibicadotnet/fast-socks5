#!/bin/sh

# Redirect all output to /dev/null để tắt hoàn toàn logs
exec > /dev/null 2>&1

# Tắt Rust logging environment variables
export RUST_LOG=off
export RUST_BACKTRACE=0
unset RUST_LOG_STYLE

# Set default values
PROXY_PORT=${PROXY_PORT:-2324}
AUTH_MODE=${AUTH_MODE:-password}
ALLOW_UDP=${ALLOW_UDP:-false}
PUBLIC_ADDR=${PUBLIC_ADDR:-}
REQUEST_TIMEOUT=${REQUEST_TIMEOUT:-10}
SKIP_AUTH=${SKIP_AUTH:-false}

# Base args
ARGS="--listen-addr 0.0.0.0:${PROXY_PORT} --request-timeout ${REQUEST_TIMEOUT}"

# Thêm UDP nếu được bật
if [ "$ALLOW_UDP" = "true" ] && [ -n "$PUBLIC_ADDR" ]; then
    ARGS="$ARGS --allow-udp --public-addr $PUBLIC_ADDR"
fi

# Thêm skip-auth nếu được bật (chỉ cho no-auth)
if [ "$SKIP_AUTH" = "true" ]; then
    ARGS="$ARGS --skip-auth"
fi

# Build command based on auth mode
if [ "$AUTH_MODE" = "no-auth" ]; then
    exec /usr/local/bin/fast-socks5-server $ARGS no-auth > /dev/null 2>&1
else
    if [ -z "$PROXY_USER" ] || [ -z "$PROXY_PASSWORD" ]; then
        exit 1
    fi
    exec /usr/local/bin/fast-socks5-server $ARGS password \
        --username "$PROXY_USER" --password "$PROXY_PASSWORD" > /dev/null 2>&1
fi
