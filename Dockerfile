
FROM rust:1.75-alpine as builder

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

# Build the server example with optimizations
RUN cargo build --release --example server

# Runtime stage - minimal Alpine image
FROM alpine:latest

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    && addgroup -g 1000 socks5 \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5

# Copy the compiled binary
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server

# Create entrypoint script
RUN cat > /usr/local/bin/entrypoint.sh << 'EOF'
#!/bin/sh

# Set default values if not provided
PROXY_USER=${PROXY_USER:-admin}
PROXY_PASSWORD=${PROXY_PASSWORD:-password}
PROXY_PORT=${PROXY_PORT:-1080}

# Start the server with environment variables
exec /usr/local/bin/fast-socks5-server \
    --listen-addr "0.0.0.0:${PROXY_PORT}" \
    --username "${PROXY_USER}" \
    --password "${PROXY_PASSWORD}" \
    2>/dev/null
EOF

# Make entrypoint executable
RUN chmod +x /usr/local/bin/entrypoint.sh

# Switch to non-root user
USER socks5

# Expose default port (will be overridden by PROXY_PORT)
EXPOSE 1080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD nc -z localhost ${PROXY_PORT:-1080} || exit 1

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
