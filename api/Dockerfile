FROM rust:1.91-slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
ARG APP_BUILD_SHA
ARG APP_BUILD_TIMESTAMP
RUN APP_BUILD_SHA="${APP_BUILD_SHA}" APP_BUILD_TIMESTAMP="${APP_BUILD_TIMESTAMP}" cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mini-crates-api /usr/local/bin/mini-crates-api

EXPOSE 3333
CMD ["mini-crates-api"]
