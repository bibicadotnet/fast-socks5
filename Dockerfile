FROM rust:1.87.0-alpine3.22 AS builder

RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /app

COPY . .

RUN rustup target add x86_64-unknown-linux-musl

RUN RUSTFLAGS="-C target-feature=+crt-static" \
    cargo build --release --target x86_64-unknown-linux-musl --examples && \
    strip /app/target/x86_64-unknown-linux-musl/release/examples/server && \
    strip /app/target/x86_64-unknown-linux-musl/release/examples/docker_server

FROM scratch

COPY --from=alpine:3.19.7 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/server /usr/local/bin/fast-socks5-server
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/docker_server /usr/local/bin/docker_server

USER 65534:65534

ENTRYPOINT ["/usr/local/bin/docker_server"]
