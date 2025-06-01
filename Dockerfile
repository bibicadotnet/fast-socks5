# Multi-stage build với multiple examples
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache --quiet \
    musl-dev \
    pkgconfig \
    openssl-dev

WORKDIR /app
COPY . .

# Build multiple examples
RUN cargo build --release --example server --quiet 2>/dev/null && \
    cargo build --release --example custom_auth_server --quiet 2>/dev/null && \
    cargo build --release --example router --quiet 2>/dev/null && \
    cargo build --release --example client --quiet 2>/dev/null && \
    cargo build --release --example udp_client --quiet 2>/dev/null

# Runtime stage
FROM alpine:latest

RUN apk add --no-cache --quiet ca-certificates && \
    addgroup -g 1000 socks5 2>/dev/null && \
    adduser -D -s /bin/sh -u 1000 -G socks5 socks5 2>/dev/null

# Copy all binaries
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server
COPY --from=builder /app/target/release/examples/custom_auth_server /usr/local/bin/fast-socks5-custom-auth
COPY --from=builder /app/target/release/examples/router /usr/local/bin/fast-socks5-router
COPY --from=builder /app/target/release/examples/client /usr/local/bin/fast-socks5-client
COPY --from=builder /app/target/release/examples/udp_client /usr/local/bin/fast-socks5-udp-client

# Copy advanced entrypoint
COPY entrypoint-advanced.sh /usr/local/bin/entrypoint.sh

RUN chmod +x /usr/local/bin/fast-socks5-* /usr/local/bin/entrypoint.sh

USER socks5
EXPOSE 2324
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
