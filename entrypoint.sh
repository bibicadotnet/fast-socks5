#!/bin/bash

# Redirect all output to /dev/null để tắt hoàn toàn logs
exec > /dev/null 2>&1

# Set default values
PROXY_PORT=${PROXY_PORT:-2324}
AUTH_MODE=${AUTH_MODE:-password}

# Build command based on auth mode
if [ "$AUTH_MODE" = "no-auth" ]; then
    # Chạy với no-auth mode, tắt hoàn toàn logs
    exec /usr/local/bin/fast-socks5-server \
        --listen-addr "0.0.0.0:${PROXY_PORT}" \
        no-auth \
        > /dev/null 2>&1
else
    # Check if credentials are provided (silent check)
    if [ -z "$PROXY_USER" ] || [ -z "$PROXY_PASSWORD" ]; then
        # Exit silently without any error messages
        exit 1
    fi
    
    # Chạy với username/password auth, tắt hoàn toàn logs
    exec /usr/local/bin/fast-socks5-server \
        --listen-addr "0.0.0.0:${PROXY_PORT}" \
        password \
        --username "${PROXY_USER}" \
        --password "${PROXY_PASSWORD}" \
        > /dev/null 2>&1
fi
