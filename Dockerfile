# Highly optimized Dockerfile for fast-socks5 server
FROM rust:1.75-alpine AS builder

# Install minimal build dependencies
RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app

# Copy all source code (including examples/docker_server.rs)
COPY . .

# Build with aggressive size optimizations
ENV RUSTFLAGS="-C opt-level=z -C target-cpu=generic -C panic=abort -C codegen-units=1 -C lto=fat"
RUN cargo build --release --example server --quiet && \
    cargo build --release --example docker_server --quiet && \
    strip target/release/examples/server && \
    strip target/release/examples/docker_server

# Use static distroless for smallest size
FROM gcr.io/distroless/static-debian12

# Copy both binaries
COPY --from=builder /app/target/release/examples/server /fast-socks5-server
COPY --from=builder /app/target/release/examples/docker_server /docker-entrypoint

# Use nonroot user from distroless
USER nonroot:nonroot

# Use Rust entrypoint wrapper for env vars support
ENTRYPOINT ["/docker-entrypoint"]
