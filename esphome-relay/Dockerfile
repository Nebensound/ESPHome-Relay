# Multi-stage build: Rust compile → minimal runtime
# Stage 1: Build
FROM rust:1.87-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/

RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/esphome-relay /usr/local/bin/esphome-relay
COPY run.sh /

RUN chmod +x /run.sh

CMD ["/run.sh"]
