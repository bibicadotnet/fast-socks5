# Multi-stage build để tối ưu kích thước image
FROM rust:1.75-alpine AS builder

# Install dependencies for building (bỏ git vì đã có source)
RUN apk add --no-cache --quiet \
    musl-dev \
    pkgconfig \
    openssl-dev

# Set working directory
WORKDIR /app

# Copy source code (thay vì git clone)
COPY . .

# Build with release optimization
RUN set -ex && \
    cargo build --release --example server --quiet && \
    cargo build --release --example custom_auth_server --quiet && \
    cargo build --release --example router --quiet && \
    cargo build --release --example client --quiet && \
    cargo build --release --example udp_client --quiet

# Runtime stage - Alpine minimal nhưng đầy đủ tính năng
FROM alpine:latest

# Install minimal runtime dependencies
RUN apk add --no-cache --quiet \
    ca-certificates \
    && addgroup -g 1000 socks5 2>/dev/null \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5 2>/dev/null

# Copy the binary from builder
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server
COPY --from=builder /app/target/release/examples/custom_auth_server /usr/local/bin/fast-socks5-custom-auth
COPY --from=builder /app/target/release/examples/router /usr/local/bin/fast-socks5-router
COPY --from=builder /app/target/release/examples/client /usr/local/bin/fast-socks5-client
COPY --from=builder /app/target/release/examples/udp_client /usr/local/bin/fast-socks5-udp-client

# Copy entrypoint script
COPY entrypoint.sh /usr/local/bin/entrypoint.sh

# Make both executable
RUN chmod +x /usr/local/bin/fast-socks5-* /usr/local/bin/entrypoint.sh

# Switch to non-root user
USER socks5

# Expose port 2324
EXPOSE 2324

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
