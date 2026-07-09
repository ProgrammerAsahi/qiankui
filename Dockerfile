FROM rust:1.97-bookworm@sha256:7d0723df719e7f213b69dc7c8c595985c3f4b060cfbee4f7bc0e347a86fe3b6a AS build

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked --bin qiankui-relay

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:ce0d66bc0f64aae46e6a03add867b07f42cc7b8799c949c2e898057b7f75a151

LABEL org.opencontainers.image.title="Qiankui Relay" \
      org.opencontainers.image.description="A private networking relay with replaceable transports" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.source="https://github.com/ProgrammerAsahi/qiankui"

COPY --from=build --chown=nonroot:nonroot /src/target/release/qiankui-relay /usr/local/bin/qiankui-relay

USER nonroot:nonroot
EXPOSE 8443/tcp
ENTRYPOINT ["/usr/local/bin/qiankui-relay"]
