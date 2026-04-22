FROM rust:1.86-slim-bookworm AS builder
WORKDIR /app

ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_INCREMENTAL=0 \
    CARGO_TERM_COLOR=always

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential ca-certificates clang libclang-dev pkg-config libssl-dev libsystemd-dev git && \
    rm -rf /var/lib/apt/lists/*

COPY .rust-toolchain.toml rust-toolchain.toml
RUN rustup toolchain install

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,sharing=private,target=/app/target \
    cargo build --profile maxperf --bin blutgang && \
    install -m 0555 /app/target/maxperf/blutgang /usr/local/bin/blutgang

# ---- runtime ----
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates openssl && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir /app
WORKDIR /app

COPY --from=builder /usr/local/bin/blutgang /app/blutgang

CMD ["./blutgang", "-c", "config.toml"]
