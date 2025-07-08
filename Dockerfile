FROM rust:1.88-trixie-slim as builder
WORKDIR /usr/src/snowflake-id-worker
COPY src/ src/
COPY Cargo.toml Cargo.lock ./

RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/myapp /usr/local/bin/snowflake-id-worker

EXPOSE 80
CMD ["snowflake-id-worker"]
