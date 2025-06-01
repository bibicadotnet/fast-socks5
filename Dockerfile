# Build stage với static linking
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

# Add musl target
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

# Copy source
COPY . .

# Build static binary (remove problematic flags first)
ENV RUSTFLAGS="-C opt-level=z -C target-cpu=generic -C panic=abort -C codegen-units=1 -C lto=fat"
RUN cargo build --release --example server --target x86_64-unknown-linux-musl --quiet && \
    cargo build --release --example docker_server --target x86_64-unknown-linux-musl --quiet && \
    strip target/x86_64-unknown-linux-musl/release/examples/server && \
    strip target/x86_64-unknown-linux-musl/release/examples/docker_server

# Runtime stage - scratch (0 MB base)
FROM scratch

# Copy CA certs for HTTPS
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy static binaries
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/server /fast-socks5-server
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/docker_server /docker-entrypoint

# Use numeric user ID (no /etc/passwd in scratch)
USER 65534:65534

ENTRYPOINT ["/docker-entrypoint"]
