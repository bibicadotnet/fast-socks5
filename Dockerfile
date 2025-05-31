# Multi-stage build để tối ưu kích thước image
FROM rust:1.75-alpine AS builder

# Install dependencies for building
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev \
    git \
    file

# Set working directory
WORKDIR /app

# Clone fast-socks5 repository
RUN git clone https://github.com/dizda/fast-socks5.git . 2>/dev/null

# Build without any debug output
RUN cargo build --release --example server --quiet 2>/dev/null

# Runtime stage - minimal Alpine image
FROM alpine:latest

# Install runtime dependencies without output
RUN apk add --no-cache --quiet \
    ca-certificates \
    bash \
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

# Set entrypoint with complete log suppression
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
