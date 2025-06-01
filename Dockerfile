FROM rust:1.75-alpine AS builder

RUN apk add --no-cache --quiet \
    musl-dev \
    pkgconfig \
    openssl-dev
    
WORKDIR /app

COPY . .

# Build with release optimization
RUN cargo build --release --example server --quiet 2>/dev/null

FROM alpine:latest

RUN apk add --no-cache --quiet \
    ca-certificates \
    && addgroup -g 1000 socks5 2>/dev/null \
    && adduser -D -s /bin/sh -u 1000 -G socks5 socks5 2>/dev/null

COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server

COPY entrypoint.sh /usr/local/bin/entrypoint.sh

RUN chmod +x /usr/local/bin/fast-socks5-server && \
    chmod +x /usr/local/bin/entrypoint.sh

USER socks5

EXPOSE 2324

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
