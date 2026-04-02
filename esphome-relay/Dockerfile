# Multi-stage build: Cross-compile Rust → minimal runtime
# Stage 1: Build (always runs on build host, cross-compiles for target)
FROM --platform=$BUILDPLATFORM rust:1.94-bookworm AS builder

ARG TARGETPLATFORM

# Install cargo-zigbuild + zig for cross-compilation, determine Rust target
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && pip3 install ziglang --break-system-packages \
    && cargo install cargo-zigbuild \
    && case "$TARGETPLATFORM" in \
    linux/amd64)  echo "x86_64-unknown-linux-gnu" ;; \
    linux/arm64)  echo "aarch64-unknown-linux-gnu" ;; \
    linux/arm/v7) echo "armv7-unknown-linux-gnueabihf" ;; \
    esac > /rust_target \
    && rustup target add $(cat /rust_target)

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/

RUN cargo zigbuild --release --target $(cat /rust_target) \
    && cp target/$(cat /rust_target)/release/esphome-relay /esphome-relay

# Stage 2: Runtime
FROM debian:bookworm-slim

ARG BUILD_VERSION=dev

LABEL \
    io.hass.name="ESPHome Firmware Relay" \
    io.hass.description="Lokaler Proxy/Cache für ESPHome OTA-Updates aus privaten GitHub Repos" \
    io.hass.version="${BUILD_VERSION}" \
    io.hass.type="addon" \
    io.hass.arch="amd64|aarch64|armv7"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /esphome-relay /usr/local/bin/esphome-relay
COPY run.sh /

RUN chmod +x /run.sh

CMD ["/run.sh"]
