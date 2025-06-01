FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app

# Copy project files
COPY Cargo.toml ./
COPY src ./src
COPY examples ./examples

# Build statically with musl
RUN rustup target add x86_64-unknown-linux-musl && \
    cargo build --release --example server --target x86_64-unknown-linux-musl && \
    cargo build --release --example docker_server --target x86_64-unknown-linux-musl

# Strip binaries to reduce size
RUN strip target/x86_64-unknown-linux-musl/release/examples/server && \
    strip target/x86_64-unknown-linux-musl/release/examples/docker_server

# Minimal final image
FROM scratch

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/server /fast-socks5-server
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/docker_server /entrypoint

ENTRYPOINT ["/entrypoint"]
