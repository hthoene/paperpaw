# plugin-host-libs

A few first-party plugins (stats, battery, keys) as x86-64 shared libraries
(`code-host.so`) for the emulator's interface-7 loader (#2850), to check
install and heap on the X4 profile. Its own workspace; the build settings in
`.cargo/config.toml` are needed (never set `RUSTFLAGS`).

    cd tools/plugin-host-libs
    # --target keeps the build settings off the build scripts:
    cargo build --release --examples --target x86_64-unknown-linux-gnu
    # -> target/x86_64-unknown-linux-gnu/release/examples/lib<id>.so
    # then, per plugin: <library>/plugins/<id>/code-host.so + plugin.ini
    paperpaw-emu --profile xteink-x4 --headless --library <library> \
        --abi7-dev-loader --trust-code --script wait:300

The emulator's dev loader offers the data services and `core.device` only, so
a plugin that needs more (stats and keys need `core.library`) is refused with
`Metadata`; battery loads.
