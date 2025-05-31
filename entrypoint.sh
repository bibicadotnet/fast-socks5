#!/bin/bash
set -e

# Set default values
PROXY_USER=${PROXY_USER:-admin}
PROXY_PASSWORD=${PROXY_PASSWORD:-password}
PROXY_PORT=${PROXY_PORT:-1080}
AUTH_MODE=${AUTH_MODE:-password}

echo "Starting SOCKS5 server on 0.0.0.0:${PROXY_PORT}"

# Build command based on auth mode
if [ "$AUTH_MODE" = "no-auth" ]; then
    echo "Running in no-auth mode"
    exec /usr/local/bin/fast-socks5-server \
        --listen-addr "0.0.0.0:${PROXY_PORT}" \
        no-auth
else
    echo "Running with username/password auth: ${PROXY_USER}"
    exec /usr/local/bin/fast-socks5-server \
        --listen-addr "0.0.0.0:${PROXY_PORT}" \
        password \
        --username "${PROXY_USER}" \
        --password "${PROXY_PASSWORD}"
fi
