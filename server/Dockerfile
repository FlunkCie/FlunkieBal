# Build from the repository root (the server shares the game's code in core/):
#   docker build -f server/Dockerfile -t flunkiebal-server .
FROM rust:1-slim AS build
WORKDIR /src
COPY core core
COPY server server
RUN cd server && cargo build --release

FROM debian:stable-slim
COPY --from=build /src/server/target/release/flunkiebal-server /usr/local/bin/flunkiebal-server
EXPOSE 8787
# friends and messages live here: keep this on a volume
ENV FLUNKI_DATA=/data/flunkiebal-data.json
VOLUME /data
CMD ["flunkiebal-server"]
