FROM rust:1.97.0-trixie@sha256:44637ff22d0a6571a221bfaf137849711ad02ff4723dbb4736e297538f6a3e60 AS build

ARG TARGETARCH
WORKDIR /src
COPY Cargo.toml Cargo.lock LICENSE ./
COPY crates/chaffnet-core/Cargo.toml crates/chaffnet-core/Cargo.toml
COPY crates/chaffnet-eval/Cargo.toml crates/chaffnet-eval/Cargo.toml
COPY crates/chaffnet-server/Cargo.toml crates/chaffnet-server/Cargo.toml

RUN mkdir -p crates/chaffnet-core/src crates/chaffnet-eval/src crates/chaffnet-server/src && \
    printf 'pub fn placeholder() {}\n' > crates/chaffnet-core/src/lib.rs && \
    printf 'pub fn placeholder() {}\n' > crates/chaffnet-eval/src/lib.rs && \
    printf 'fn main() {}\n' > crates/chaffnet-eval/src/main.rs && \
    printf 'pub fn placeholder() {}\n' > crates/chaffnet-server/src/lib.rs && \
    printf 'fn main() {}\n' > crates/chaffnet-server/src/main.rs

RUN --mount=type=cache,id=chaffnet-cargo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    --mount=type=cache,id=chaffnet-target-rust197-trixie-${TARGETARCH},target=/src/target \
    cargo build --locked --release -p chaffnet-server

COPY crates crates
COPY models models

RUN --mount=type=cache,id=chaffnet-cargo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    --mount=type=cache,id=chaffnet-target-rust197-trixie-${TARGETARCH},target=/src/target \
    find crates -name '*.rs' -exec touch {} + && \
    cargo build --locked --release -p chaffnet-server && \
    mkdir -p /out/data && \
    cp /src/target/release/chaffnet-server /out/chaffnet-server && \
    strip /out/chaffnet-server

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:aded2458d026e046cb68199db0e5793e1028ffa143f7258f3c4278253e20add7

LABEL org.opencontainers.image.source="https://github.com/iuliandita/chaffnet" \
      org.opencontainers.image.licenses="Apache-2.0"

COPY --from=build --chown=65532:65532 /out/chaffnet-server /usr/local/bin/chaffnet-server
COPY --from=build --chown=65532:65532 /out/data /data
COPY --from=build /src/LICENSE /licenses/chaffnet/LICENSE

ENV CHAFFNET_BIND="0.0.0.0:8080" \
    CHAFFNET_DB="/data/chaffnet.redb" \
    CHAFFNET_NETWORK_DB="/data/chaffnet-network.redb"

USER 65532:65532
EXPOSE 8080
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/chaffnet-server", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/chaffnet-server"]
