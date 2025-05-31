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

# Build the server example with optimizations
RUN cargo build --release --example server

# Runtime stage - minimal Alpine image
FROM alpine:latest

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    netcat-openbsd \
    && addgroup -g 1000 socks5 \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5

# Copy the compiled binary
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server

# Create entrypoint script
RUN echo '#!/bin/sh' > /usr/local/bin/entrypoint.sh && \
    echo '' >> /usr/local/bin/entrypoint.sh && \
    echo '# Set default values if not provided' >> /usr/local/bin/entrypoint.sh && \
    echo 'PROXY_USER=${PROXY_USER:-admin}' >> /usr/local/bin/entrypoint.sh && \
    echo 'PROXY_PASSWORD=${PROXY_PASSWORD:-password}' >> /usr/local/bin/entrypoint.sh && \
    echo 'PROXY_PORT=${PROXY_PORT:-1080}' >> /usr/local/bin/entrypoint.sh && \
    echo '' >> /usr/local/bin/entrypoint.sh && \
    echo '# Start the server with environment variables' >> /usr/local/bin/entrypoint.sh && \
    echo 'exec /usr/local/bin/fast-socks5-server \' >> /usr/local/bin/entrypoint.sh && \
    echo '    --listen-addr "0.0.0.0:${PROXY_PORT}" \' >> /usr/local/bin/entrypoint.sh && \
    echo '    --username "${PROXY_USER}" \' >> /usr/local/bin/entrypoint.sh && \
    echo '    --password "${PROXY_PASSWORD}" \' >> /usr/local/bin/entrypoint.sh && \
    echo '    2>/dev/null' >> /usr/local/bin/entrypoint.sh

# Make entrypoint executable
RUN chmod +x /usr/local/bin/entrypoint.sh

# Switch to non-root user
USER socks5

# Expose default port (will be overridden by PROXY_PORT)
EXPOSE 1080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD nc -z localhost $PROXY_PORT || exit 1

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
