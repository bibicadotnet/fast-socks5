FROM rust:1.87.0-alpine3.22 AS builder

RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /app

COPY . .

ARG TARGETPLATFORM
RUN case "$TARGETPLATFORM" in \
        "linux/amd64") RUST_TARGET="x86_64-unknown-linux-musl" ;; \
        "linux/arm64") RUST_TARGET="aarch64-unknown-linux-musl" ;; \
        *) echo "Unsupported platform: $TARGETPLATFORM"; exit 1 ;; \
    esac && \
    echo "Building for $RUST_TARGET" && \
    rustup target add $RUST_TARGET && \
    RUSTFLAGS="-C target-feature=+crt-static" \
    cargo build --release --target $RUST_TARGET --example server --example docker_server && \
    strip /app/target/$RUST_TARGET/release/examples/server && \
    strip /app/target/$RUST_TARGET/release/examples/docker_server && \
    mkdir -p /app/output && \
    cp /app/target/$RUST_TARGET/release/examples/server /app/output/server && \
    cp /app/target/$RUST_TARGET/release/examples/docker_server /app/output/docker_server

FROM scratch

COPY --from=alpine:3.19.7 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

COPY --from=builder /app/output/server /usr/local/bin/fast-socks5-server
COPY --from=builder /app/output/docker_server /usr/local/bin/docker_server

USER 65534:65534

ENTRYPOINT ["/usr/local/bin/docker_server"]
