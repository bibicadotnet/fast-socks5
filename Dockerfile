# Build stage với static linking
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev build-base

# Add musl target
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

# Copy source
COPY . .

# Build static binary với RUSTFLAGS nhẹ hơn
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo build --release --example server --target x86_64-unknown-linux-musl && \
    cargo build --release --example docker_server --target x86_64-unknown-linux-musl && \
    strip target/x86_64-unknown-linux-musl/release/examples/server && \
    strip target/x86_64-unknown-linux-musl/release/examples/docker_server

# Runtime stage - scratch (0 MB base)
FROM scratch

# Copy static binaries
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/server /fast-socks5-server
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/docker_server /docker-entrypoint

# Use numeric user ID (no /etc/passwd in scratch)
USER 65534:65534

ENTRYPOINT ["/docker-entrypoint"]
