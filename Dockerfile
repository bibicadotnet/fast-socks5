# Multi-stage build for fast-socks5
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache --quiet \
    musl-dev \
    pkgconfig \
    openssl-dev

WORKDIR /app
COPY . .

# Build all examples
RUN set -ex && \
    cargo build --release --example server --quiet && \
    cargo build --release --example custom_auth_server --quiet && \
    cargo build --release --example router --quiet && \
    cargo build --release --example client --quiet && \
    cargo build --release --example udp_client --quiet

# Runtime stage
FROM alpine:latest

# Install dependencies and create user
RUN apk add --no-cache --quiet ca-certificates && \
    addgroup -g 1000 socks5 && \
    adduser -D -s /bin/sh -u 1000 -G socks5 socks5

# Copy all binaries with clear names
COPY --from=builder /app/target/release/examples/server /usr/local/bin/fast-socks5-server
COPY --from=builder /app/target/release/examples/custom_auth_server /usr/local/bin/fast-socks5-custom-auth
COPY --from=builder /app/target/release/examples/router /usr/local/bin/fast-socks5-router
COPY --from=builder /app/target/release/examples/client /usr/local/bin/fast-socks5-client
COPY --from=builder /app/target/release/examples/udp_client /usr/local/bin/fast-socks5-udp-client

# Copy entrypoint
COPY entrypoint.sh /usr/local/bin/entrypoint.sh

# Set permissions
RUN chmod +x /usr/local/bin/fast-socks5-* /usr/local/bin/entrypoint.sh

USER socks5
EXPOSE 2324
WORKDIR /home/socks5

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD []
