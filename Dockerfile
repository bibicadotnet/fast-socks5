FROM rust:1.75-alpine AS builder

RUN apk add --no-cache --quiet \
    musl-dev \
    pkgconfig \
    openssl-dev

WORKDIR /app

COPY . .

RUN cargo build --release --example server --quiet 2>/dev/null

FROM alpine:3.19.7

# Install minimal runtime dependencies
RUN apk add --no-cache --quiet \
    ca-certificates \
    && addgroup -g 1000 socks5 2>/dev/null \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5 2>/dev/null

COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server

COPY entrypoint.sh /usr/local/bin/entrypoint.sh

RUN chmod +x /usr/local/bin/fast-socks5-server && \
    chmod +x /usr/local/bin/entrypoint.sh

USER socks5

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
