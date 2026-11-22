# syntax=docker/dockerfile:1
FROM rust:1.83-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/rust-core /usr/local/bin/rust-core
ENV RUST_CORE_PORT=8080
EXPOSE 8080
CMD ["rust-core"]
