FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev binutils

# Add musl target
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

COPY . .

# OPTIONAL: Uncomment if you want fine-tuned optimization
# ENV RUSTFLAGS="-C opt-level=z -C target-cpu=generic -C panic=abort -C codegen-units=1 -C lto=fat"

# Build examples with full static linking
RUN cargo build --release --example server --target x86_64-unknown-linux-musl && \
    cargo build --release --example docker_server --target x86_64-unknown-linux-musl && \
    strip target/x86_64-unknown-linux-musl/release/examples/server && \
    strip target/x86_64-unknown-linux-musl/release/examples/docker_server

FROM scratch

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/server /fast-socks5-server
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/docker_server /docker-entrypoint

USER 65534:65534
ENTRYPOINT ["/docker-entrypoint"]
