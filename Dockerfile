# Stage 1: Build Rust static binaries with musl
FROM rust:1.87.0-alpine3.22 AS builder

# Cài dependencies
RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /app
COPY . .

# Sử dụng TARGETPLATFORM để xác định architecture
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

# Stage 2: Final scratch image (rỗng, không có OS)
FROM scratch
# Copy CA certificates nếu cần HTTPS/TLS
COPY --from=alpine:3.19.7 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
# Copy 2 binary static vào từ output directory
COPY --from=builder /app/output/server /usr/local/bin/fast-socks5-server
COPY --from=builder /app/output/docker_server /usr/local/bin/docker_server
# Chạy với nobody user (UID/GID 65534)
USER 65534:65534
# Chạy docker_server làm entrypoint
ENTRYPOINT ["/usr/local/bin/docker_server"]
