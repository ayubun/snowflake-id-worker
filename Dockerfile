FROM rust:1.88 AS builder

WORKDIR /usr/snowflake-id-worker
COPY src/ src/
COPY benches/ benches/
COPY Cargo.toml Cargo.lock ./

RUN cargo build --release


FROM debian:trixie-slim

WORKDIR /usr/local/bin
# NOTE(ayubun): ngl idk why this is valuable but it's documented on rust's docker images https://hub.docker.com/_/rust
# could prolly do without it but im lazy =w= submit a PR if u know what ur doing !!
RUN apt-get update && rm -rf /var/lib/apt/lists/*
#
COPY --from=builder /usr/snowflake-id-worker/target/release/snowflake-id-worker /usr/local/bin/snowflake-id-worker

EXPOSE 80
CMD ["snowflake-id-worker"]
