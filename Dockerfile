# Stage 1: Builder
FROM rust:1.87.0-alpine3.22 AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev pkgconfig && \
    rustup target add x86_64-unknown-linux-musl

WORKDIR /app
COPY . .

# Build with optimizations
RUN RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-s" \
    cargo build --release --target x86_64-unknown-linux-musl --examples

# Stage 2: Runtime
FROM scratch

# Copy CA certificates
COPY --from=alpine:3.19.7 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy binaries
COPY --from=builder \
    /app/target/x86_64-unknown-linux-musl/release/examples/server \
    /usr/local/bin/fast-socks5-server

COPY --from=builder \
    /app/target/x86_64-unknown-linux-musl/release/examples/docker_server \
    /usr/local/bin/docker_server

# Run as nobody user
USER 65534:65534

# Entrypoint
ENTRYPOINT ["/usr/local/bin/docker_server"]
