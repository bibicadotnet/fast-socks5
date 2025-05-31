# Multi-stage build để tối ưu kích thước image
FROM rust:1.75-alpine AS builder

# Install dependencies for building
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev \
    git

# Set working directory
WORKDIR /app

# Clone fast-socks5 repository
RUN git clone https://github.com/dizda/fast-socks5.git .

# Debug: Show what we have
RUN ls -la examples/
RUN cat examples/server.rs | head -20

# Build with verbose output to see any errors
RUN RUST_LOG=debug cargo build --release --example server --verbose

# Verify the binary was built
RUN ls -la target/release/examples/
RUN file target/release/examples/server

# Runtime stage - minimal Alpine image
FROM alpine:latest

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    netcat-openbsd \
    bash \
    && addgroup -g 1000 socks5 \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5

# Copy the binary and verify it exists
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server

# Verify copy worked
RUN ls -la /usr/local/bin/fast-socks5-server
RUN file /usr/local/bin/fast-socks5-server

# Make executable
RUN chmod +x /usr/local/bin/fast-socks5-server

# Test the binary
RUN /usr/local/bin/fast-socks5-server --help || echo "Binary exists but help failed"

# Create entrypoint script
RUN cat > /usr/local/bin/entrypoint.sh << 'EOF'
#!/bin/bash
set -e

# Debug info
echo "=== Debug Info ==="
echo "Binary exists: $(ls -la /usr/local/bin/fast-socks5-server 2>/dev/null || echo 'NOT FOUND')"
echo "Binary info: $(file /usr/local/bin/fast-socks5-server 2>/dev/null || echo 'FILE FAILED')"

# Set default values
PROXY_USER=${PROXY_USER:-admin}
PROXY_PASSWORD=${PROXY_PASSWORD:-password}
PROXY_PORT=${PROXY_PORT:-1080}

echo "Starting SOCKS5 server on 0.0.0.0:${PROXY_PORT} with user: ${PROXY_USER}"

# Try to get help first
echo "Checking server help:"
/usr/local/bin/fast-socks5-server --help || echo "Help command failed"

# Start server
exec /usr/local/bin/fast-socks5-server \
    --listen-addr "0.0.0.0:${PROXY_PORT}" \
    -u "${PROXY_USER}" \
    -p "${PROXY_PASSWORD}"
EOF

# Make entrypoint executable
RUN chmod +x /usr/local/bin/entrypoint.sh

# Switch to non-root user
USER socks5

# Expose default port
EXPOSE 1080

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
