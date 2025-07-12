FROM rust:1.88-alpine AS build

ARG PROFILE=release

WORKDIR /build

RUN mkdir bin && \
    apk add --no-cache musl-dev

# Mount Cargo's work directories as cache to allow faster
# And incremental rebuilds during the development process
RUN --mount=type=cache,target=target \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=benches,target=benches \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml,readwrite \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock,readwrite \
    cargo build --profile $PROFILE && \
    # The "dev" target directory is named "debug" instead
    [ $PROFILE = "dev" ] && FOLDER="debug" || FOLDER="release"; \
    # Move output into an unmounted directory for copying
    mv target/$FOLDER/healthcheck /build/bin && \
    mv target/$FOLDER/snowflake-id-worker /build/bin

FROM scratch AS image

ARG PROFILE

EXPOSE 80

ENV PATH=/usr/local/bin
# Non-empty value to enable
ENV RUST_BACKTRACE=$PROFILE

COPY --from=build /build/bin /usr/local/bin

HEALTHCHECK --interval=5s --start-interval=1s --retries=1 --timeout=5s --start-period=1m CMD ["healthcheck"]

USER 1000:1000
CMD ["snowflake-id-worker"]
