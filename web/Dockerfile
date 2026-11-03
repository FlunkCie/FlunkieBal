# The web version, served by nginx. Build from the repository root:
#   docker build -f web/Dockerfile -t flunkiebal-web .
FROM rust:1-slim AS build
RUN rustup target add wasm32-unknown-unknown
# must match wasm-bindgen in Cargo.lock
RUN cargo install wasm-bindgen-cli --version 0.2.130 --locked
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY core core
COPY assets assets
COPY src src
COPY web web
RUN cargo build --release --lib --target wasm32-unknown-unknown \
 && wasm-bindgen --target web --no-typescript --out-dir dist target/wasm32-unknown-unknown/release/flunkiebal.wasm \
 && V=$(sha256sum dist/flunkiebal_bg.wasm | cut -c1-12) \
 && sed "s/__BUILD__/$V/g" web/index.html > dist/index.html

FROM nginx:alpine
COPY web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /src/dist /usr/share/nginx/html
