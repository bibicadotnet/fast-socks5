#!/bin/sh
set -e

# Đọc biến môi trường
PROXY_USER="${PROXY_USER:-admin}"
PROXY_PASSWORD="${PROXY_PASSWORD:-password}"
PROXY_PORT="${PROXY_PORT:-1080}"

# Log thông tin cấu hình
echo "🟢 Starting SOCKS5 server on 0.0.0.0:${PROXY_PORT}"
echo "🔑 Auth: ${PROXY_USER}:${PROXY_PASSWORD}"

# Khởi động server
exec /usr/local/bin/fast-socks5-server \
    --listen-addr "0.0.0.0:${PROXY_PORT}" \
    password \
    --username "${PROXY_USER}" \
    --password "${PROXY_PASSWORD}"
