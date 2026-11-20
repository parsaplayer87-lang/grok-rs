# Pure-Rust dependency tree: no C toolchain, no OpenSSL, no ring/aws-lc.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --bin grok-rs

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/grok-rs /usr/local/bin/grok-rs

ENV GROK_RS_DATA_DIR=/data \
    GROK_RS_LAN=1 \
    GROK_RS_PORT=8787
VOLUME /data
EXPOSE 8787

# The host token is generated on first run and printed to stdout; set
# GROK_RS_TOKEN to pin it instead.
ENTRYPOINT ["grok-rs"]
CMD ["serve"]