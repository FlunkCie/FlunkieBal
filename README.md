# FlunkieBal

A mobile game of **flunkyball**, the Dutch park drinking game: two teams, a bottle in the
middle, and one throw at a time. Knock the bottle over and your team drinks while the other
team runs to set it up again. First team to finish their drinks wins.

Made for **[FlunkCie](https://flunkcie.nl)**, the official FlunkyBal committee of
AEGEE-Amsterdam. Rules, locations and tone follow FlunkCie.

## Features

- Play against the computer, practise in drills, or learn the game in the tutorial
- Online matches with friends, run on a central server (`wss://flunkiebal.flunkcie.nl`)
- Arenas: Vondelpark (day and evening), the Roof, the Café, the Duplex, Krakau Hostel, plus the
  clubhouse and training hall
- Friends, quick messages and emotes; levels unlock cosmetics as you play
- Runs on Android, in the browser (WebAssembly + WebGL2) and natively on the desktop

## Building

Written in Rust with [wgpu](https://wgpu.rs) and [egui](https://github.com/emilk/egui) (eframe 0.37).

| Target  | Command |
|---------|---------|
| Desktop | `cargo run --release` |
| Web     | `./build-web.sh`, then `python3 -m http.server -d dist 8000` |
| Android | open `android/` in Android Studio, or `cd android && gradle assembleDebug` |
| Server  | `cd server && cargo run --release -- 8787` |

Requirements:

- **Web**: `rustup target add wasm32-unknown-unknown` and
  `cargo install wasm-bindgen-cli --version 0.2.130` (must match `Cargo.lock`)
- **Android**: the Android NDK and `rustup target add aarch64-linux-android x86_64-linux-android`.
  Set `FLUNKI_ABIS=x86_64` to build only the emulator's library.

To point the desktop build at a local server instead of the public one:

```sh
FLUNKI_SERVER=ws://127.0.0.1:8787 cargo run --release
```

### Running the server with Docker

```sh
docker compose up -d --build
```

This starts the game server and the web version (nginx on port 7878, which also forwards
websockets to the server, so a TLS proxy only needs to point at 7878). Data is kept on the
`flunkiebal-data` volume. To build just the server image:
`docker build -f server/Dockerfile -t flunkiebal-server .`

## Tests

```sh
cargo test --release --lib            # client, incl. shader validation for WebGL2
(cd core && cargo test --release)     # rules, mirroring, prediction
(cd server && cargo test --release)   # real server with bots over websockets (~40 s)
```

## Project layout

```
core/      rules, physics, CPU players, animation and the online protocol (shared with the server)
src/       the game: screens, input, camera, renderer, arenas, audio, networking, Android glue
server/    flunkiebal-server: rooms, matches, friends and accounts
web/       page shell, single-file page builder, Dockerfile and nginx config
android/   Gradle project (nl.flunkcie.flunkiebal, Android 8+)
assets/    character model and fonts
```

See [`CLAUDE.md`](CLAUDE.md) for a detailed tour of the code and the pitfalls to know about.

## License

FlunkieBal is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of
the License, or (at your option) any later version. See [`LICENSE`](LICENSE).

The bundled assets keep their own licenses (see Credits).

## Credits

- Character model by [Quaternius](https://quaternius.com)
- Icons: [Phosphor](https://phosphoricons.com) (see `assets/Phosphor-LICENSE.txt`)
- Fonts: Anton and Barlow Condensed
