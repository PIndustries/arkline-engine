# Build from the repository root with -f deploy/Dockerfile.
FROM rust:1.89-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p arkline-server

FROM debian:bookworm-slim
ARG SOURCE_COMMIT=unknown
LABEL org.opencontainers.image.title="Arkline authoritative server" \
      org.opencontainers.image.revision="${SOURCE_COMMIT}"
WORKDIR /data
COPY --from=build /src/target/release/arkline-server /usr/local/bin/arkline-server
COPY LICENSE TRADEMARKS.md /usr/share/doc/arkline/
USER 10001:10001
EXPOSE 7777 7778
ENTRYPOINT ["/usr/local/bin/arkline-server"]
CMD ["--bind", "0.0.0.0:7777", "--db", "/data/world.sqlite"]
