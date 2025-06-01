FROM rust:1.75-alpine AS builder

RUN apk add --no-cache --quiet \
    musl-dev \
    pkgconfig \
    openssl-dev

WORKDIR /app
COPY . .

RUN RUSTFLAGS="-C link-arg=-s" \
    cargo build --release --example server --quiet 2>/dev/null && \
    strip /app/target/release/examples/server

FROM alpine:3.19.7

RUN apk add --no-cache --quiet ca-certificates && \
    addgroup -g 1000 socks5 && \
    adduser -D -s /bin/sh -u 1000 -G socks5 socks5

COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server
COPY --chmod=755 entrypoint.sh /usr/local/bin/entrypoint.sh

USER socks5
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
