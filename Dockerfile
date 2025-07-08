FROM rust:1.88 as builder
WORKDIR /usr/src/snowflake-id-worker
COPY src/ src/
COPY Cargo.toml Cargo.lock ./

RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/myapp /usr/local/bin/snowflake-id-worker

EXPOSE 80
CMD ["snowflake-id-worker"]
