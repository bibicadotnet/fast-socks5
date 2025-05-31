#!/bin/bash
set -e

# Set default values
PROXY_PORT=${PROXY_PORT:-2324}
AUTH_MODE=${AUTH_MODE:-password}

echo "Starting SOCKS5 server on 0.0.0.0:${PROXY_PORT}"

# Build command based on auth mode
if [ "$AUTH_MODE" = "no-auth" ]; then
    echo "Running in no-auth mode"
    exec /usr/local/bin/fast-socks5-server \
        --listen-addr "0.0.0.0:${PROXY_PORT}" \
        no-auth
else
    # Check if credentials are provided
    if [ -z "$PROXY_USER" ] || [ -z "$PROXY_PASSWORD" ]; then
        echo "ERROR: Username and password are required!"
        echo ""
        echo "You must set both environment variables:"
        echo "  PROXY_USER=your_username"
        echo "  PROXY_PASSWORD=your_password"
        echo ""
        echo "Example:"
        echo "  docker run -e PROXY_USER=myuser -e PROXY_PASSWORD=mypass -p 2324:2324 your-image"
        echo ""
        echo "Or use no-auth mode (not recommended for production):"
        echo "  docker run -e AUTH_MODE=no-auth -p 2324:2324 your-image"
        exit 1
    fi
    
    echo "Running with username/password auth: ${PROXY_USER}"
    exec /usr/local/bin/fast-socks5-server \
        --listen-addr "0.0.0.0:${PROXY_PORT}" \
        password \
        --username "${PROXY_USER}" \
        --password "${PROXY_PASSWORD}"
fi
