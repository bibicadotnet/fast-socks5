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
RUN git clone https://github.com/dizda/fast-socks5.git .

# Build with verbose output to see any errors
RUN RUST_LOG=debug cargo build --release --example server --verbose

# Verify the binary was built
RUN ls -la target/release/examples/server && echo "Binary size: $(du -h target/release/examples/server)"

# Runtime stage - minimal Alpine image
FROM alpine:latest

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    bash \
    && addgroup -g 1000 socks5 \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5

# Copy the binary from builder
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server

# Copy entrypoint script
COPY entrypoint.sh /usr/local/bin/entrypoint.sh

# Make both executable
RUN chmod +x /usr/local/bin/fast-socks5-server && \
    chmod +x /usr/local/bin/entrypoint.sh

# Test the binary
RUN /usr/local/bin/fast-socks5-server --help

# Switch to non-root user
USER socks5

# Expose port 2324
EXPOSE 2324

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
