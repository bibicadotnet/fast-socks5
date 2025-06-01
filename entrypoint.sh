#!/bin/sh
#
# Simple entrypoint for fast-socks5 Docker image
# 
# Usage:
#   Default: docker run image_name
#   Custom:  docker run image_name /usr/local/bin/fast-socks5-custom-auth [options]
#   Direct:  docker run image_name [any_command_with_args]
#

# Silent mode - disable all logging completely
export RUST_LOG=off
export RUST_BACKTRACE=0
exec > /dev/null 2>&1

# If arguments provided, execute them directly (silently)
if [ $# -gt 0 ]; then
    exec "$@"
fi

# Default: Basic SOCKS5 server with no authentication
# Users can override this by passing arguments
exec /usr/local/bin/fast-socks5-server \
    --listen-addr "0.0.0.0:2324" \
    no-auth
