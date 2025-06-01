#!/bin/sh

# Redirect all output to /dev/null để tắt hoàn toàn logs
exec > /dev/null 2>&1

# Tắt Rust logging
export RUST_LOG=off
export RUST_BACKTRACE=0
unset RUST_LOG_STYLE

# Set default values
PROXY_PORT=${PROXY_PORT:-2324}
AUTH_MODE=${AUTH_MODE:-password}
SERVER_MODE=${SERVER_MODE:-basic}  # basic, custom_auth, router, client, udp_client

case "$SERVER_MODE" in
    "basic")
        # Basic SOCKS5 server (như cũ)
        if [ "$AUTH_MODE" = "no-auth" ]; then
            exec /usr/local/bin/fast-socks5-server \
                --listen-addr "0.0.0.0:${PROXY_PORT}" \
                no-auth > /dev/null 2>&1
        else
            if [ -z "$PROXY_USER" ] || [ -z "$PROXY_PASSWORD" ]; then
                exit 1
            fi
            exec /usr/local/bin/fast-socks5-server \
                --listen-addr "0.0.0.0:${PROXY_PORT}" \
                password \
                --username "${PROXY_USER}" \
                --password "${PROXY_PASSWORD}" > /dev/null 2>&1
        fi
        ;;
    
    "custom_auth")
        # Custom authentication server
        exec /usr/local/bin/fast-socks5-custom-auth \
            --listen-addr "0.0.0.0:${PROXY_PORT}" > /dev/null 2>&1
        ;;
    
    "router")
        # Router mode - advanced traffic routing
        ROUTER_CONFIG=${ROUTER_CONFIG:-""}
        if [ -n "$ROUTER_CONFIG" ]; then
            exec /usr/local/bin/fast-socks5-router \
                --listen-addr "0.0.0.0:${PROXY_PORT}" \
                --config "$ROUTER_CONFIG" > /dev/null 2>&1
        else
            exec /usr/local/bin/fast-socks5-router \
                --listen-addr "0.0.0.0:${PROXY_PORT}" > /dev/null 2>&1
        fi
        ;;
    
    "client")
        # Client mode - để kết nối đến SOCKS5 server khác
        TARGET_HOST=${TARGET_HOST:-"127.0.0.1"}
        TARGET_PORT=${TARGET_PORT:-"1080"}
        if [ -n "$PROXY_USER" ] && [ -n "$PROXY_PASSWORD" ]; then
            exec /usr/local/bin/fast-socks5-client \
                --server "${TARGET_HOST}:${TARGET_PORT}" \
                --username "$PROXY_USER" \
                --password "$PROXY_PASSWORD" > /dev/null 2>&1
        else
            exec /usr/local/bin/fast-socks5-client \
                --server "${TARGET_HOST}:${TARGET_PORT}" > /dev/null 2>&1
        fi
        ;;
    
    "udp_client")
        # UDP client mode
        TARGET_HOST=${TARGET_HOST:-"127.0.0.1"}
        TARGET_PORT=${TARGET_PORT:-"1080"}
        exec /usr/local/bin/fast-socks5-udp-client \
            --server "${TARGET_HOST}:${TARGET_PORT}" > /dev/null 2>&1
        ;;
    
    *)
        # Default fallback to basic mode
        exec /usr/local/bin/fast-socks5-server \
            --listen-addr "0.0.0.0:${PROXY_PORT}" \
            no-auth > /dev/null 2>&1
        ;;
esac
