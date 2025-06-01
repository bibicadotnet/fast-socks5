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
RUN cargo build --release --example server --quiet 2>/dev/null

# Runtime stage - Alpine minimal nhưng đầy đủ tính năng
FROM alpine:3.19.1

# Install minimal runtime dependencies
RUN apk add --no-cache --quiet \
    ca-certificates \
    && addgroup -g 1000 socks5 2>/dev/null \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5 2>/dev/null

# Copy the binary from builder
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server

# Copy entrypoint script
COPY entrypoint.sh /usr/local/bin/entrypoint.sh

# Make both executable
RUN chmod +x /usr/local/bin/fast-socks5-server && \
    chmod +x /usr/local/bin/entrypoint.sh

# Switch to non-root user
USER socks5

# Expose port 2324
EXPOSE 2324

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
