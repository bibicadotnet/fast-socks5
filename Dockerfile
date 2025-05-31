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
RUN echo '#!/bin/bash' > /usr/local/bin/entrypoint.sh && \
    echo 'set -e' >> /usr/local/bin/entrypoint.sh && \
    echo '' >> /usr/local/bin/entrypoint.sh && \
    echo '# Debug info' >> /usr/local/bin/entrypoint.sh && \
    echo 'echo "=== Debug Info ==="' >> /usr/local/bin/entrypoint.sh && \
    echo 'echo "Binary exists: $(ls -la /usr/local/bin/fast-socks5-server 2>/dev/null || echo '"'"'NOT FOUND'"'"')"' >> /usr/local/bin/entrypoint.sh && \
    echo 'echo "Binary info: $(file /usr/local/bin/fast-socks5-server 2>/dev/null || echo '"'"'FILE FAILED'"'"')"' >> /usr/local/bin/entrypoint.sh && \
    echo '' >> /usr/local/bin/entrypoint.sh && \
    echo '# Set default values' >> /usr/local/bin/entrypoint.sh && \
    echo 'PROXY_USER=${PROXY_USER:-admin}' >> /usr/local/bin/entrypoint.sh && \
    echo 'PROXY_PASSWORD=${PROXY_PASSWORD:-password}' >> /usr/local/bin/entrypoint.sh && \
    echo 'PROXY_PORT=${PROXY_PORT:-1080}' >> /usr/local/bin/entrypoint.sh && \
    echo '' >> /usr/local/bin/entrypoint.sh && \
    echo 'echo "Starting SOCKS5 server on 0.0.0.0:${PROXY_PORT} with user: ${PROXY_USER}"' >> /usr/local/bin/entrypoint.sh && \
    echo '' >> /usr/local/bin/entrypoint.sh && \
    echo '# Try to get help first' >> /usr/local/bin/entrypoint.sh && \
    echo 'echo "Checking server help:"' >> /usr/local/bin/entrypoint.sh && \
    echo '/usr/local/bin/fast-socks5-server --help || echo "Help command failed"' >> /usr/local/bin/entrypoint.sh && \
    echo '' >> /usr/local/bin/entrypoint.sh && \
    echo '# Start server' >> /usr/local/bin/entrypoint.sh && \
    echo 'exec /usr/local/bin/fast-socks5-server \' >> /usr/local/bin/entrypoint.sh && \
    echo '    --listen-addr "0.0.0.0:${PROXY_PORT}" \' >> /usr/local/bin/entrypoint.sh && \
    echo '    -u "${PROXY_USER}" \' >> /usr/local/bin/entrypoint.sh && \
    echo '    -p "${PROXY_PASSWORD}"' >> /usr/local/bin/entrypoint.sh

# Make entrypoint executable
RUN chmod +x /usr/local/bin/entrypoint.sh

# Switch to non-root user
USER socks5

# Expose default port
EXPOSE 1080

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
