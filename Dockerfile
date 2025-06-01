FROM rust:1.75-alpine AS builder

RUN apk add --no-cache --quiet musl-dev pkgconfig openssl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples

RUN cargo build --release --example server

# Kiểm tra binary tồn tại trước khi copy
RUN test -f /app/target/release/examples/server || (echo "Error: Binary not found"; exit 1)

# Stage 2: Runtime
FROM alpine:3.19.1

RUN apk add --no-cache --quiet --no-progress ca-certificates && \
    addgroup -g 1000 socks5 && \
    adduser -D -s /bin/sh -u 1000 -G socks5 socks5

COPY --from=builder --chown=socks5:socks5 /app/target/release/examples/server /usr/local/bin/fast-socks5-server
COPY --chown=socks5:socks5 entrypoint.sh /usr/local/bin/entrypoint.sh

RUN chmod +x /usr/local/bin/fast-socks5-server /usr/local/bin/entrypoint.sh

USER socks5

EXPOSE 2324

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
