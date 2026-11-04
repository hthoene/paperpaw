# The sample plugin as a file

The SDK's sample plugin ([`../hello-plugin`](../hello-plugin)), unchanged,
as a native plugin file (`code.ppn`) that the Xteink X4 and X4 Pro load
at their start, without a new firmware (ADR 0024,
[Plugins as files](../../docs/plugins.md#plugins-as-files)). It is a
workspace of its own, as a plugin's own repository is, to start one
from:

| | What |
|---|---|
| `src/main.rs` | the file's crate: one line, `plugin!` around the plugin |
| `.cargo/config.toml` | how a plugin is linked, for both chips, and the aliases `x4` and `x4pro` |
| `rust-toolchain.toml` | the compiler for the X4 (the X4 Pro's comes from espup) |
| `plugin/plugin.ini` | the plugin's folder, to which `code.ppn` is added |
| `plugin/store/store.ini` | what the plugin store shows of it, for the upload form; readers never install `store/` |
| `build.sh` | builds both files, packs and signs them, zips the folder, into `dist/` |
| `.github/workflows/build.yml` | a CI job for a plugin's repository: `build.sh` on every tag, its files as a release |
| `.gitignore` | keeps `target/`, `dist/` and any `*.key` out of the repository |
| `examples/second.rs` | a second plugin, to try several at once |
| `examples/crash.rs` | a plugin that panics at every start, to try safe mode |

Here it uses the SDK from this repository; a plugin's own repository
names the release it is built for (`paperpaw-sdk = "0.6.0-beta.2"`).

## Building, packing, trying

```sh
cargo x4                  # the X4: riscv32imc-unknown-none-elf
cargo +esp x4pro          # the X4 Pro: xtensa-esp32s3-none-elf, espup's toolchain
cargo x4 --example crash  # an example, the same way
```

The X4 Pro's build needs the core libraries of the PaperPaw release in
the `esp` toolchain, installed once for each release
(`tools/xtensa-std/install.sh paperpaw-xtensa-std-<version>.tar.gz`, see
[its README](../../tools/xtensa-std/README.md)); the X4's come from
rustup.

Then the plugin's folder with the file in it, signed with the author's
key (`paperpaw-sign key author.key` makes one), onto the device over
the USB cable (from the repository's top). The device shows what comes
and installs it once you say yes there:

```sh
cp -r samples/hello-native/plugin hello
cargo run -p paperpaw-sign -- pack samples/hello-native/target/riscv32imc-unknown-none-elf/release/hello hello/code.ppn --key author.key
python tools/usb-transfer.py install hello
```

The file runs only on a PaperPaw built by the same compiler from the
same SDK; *About* on the device names both. And only once installed: a
file copied into a plugin's folder does not run until it is named as
installed, as the store, the web flasher and the transfer over USB or
Wi-Fi do ([docs/usb-transfer.md](../../docs/usb-transfer.md)). A plugin
installed so is local: the store does not update it.

## Publishing

`build.sh` builds the files as the plugin library rebuilds them to
compare. It writes paths the same in any folder: the source's top as
`/plugin` and Cargo's home as `/cargo`. The X4 Pro's core libraries
come built, with their paths as `/rust`. The CI job runs `build.sh`.
Push a tag `v` followed by `plugin.ini`'s version, and keep the
author's key as the repository's secret `PAPERPAW_PLUGIN_KEY`. The
tag's release then holds the folder as two ZIPs: one with `store/` for
the upload form, one without it for the catalogue. It also holds a
signed file for each reader and their SHA-256 sums. You can do the same
on your own computer with Linux or macOS. You need both toolchains and
`paperpaw-sign` of the release:

```sh
. ~/export-esp.sh
./build.sh --key ~/.paperpaw/author.key
```

[Publishing](../../docs/plugins.md#publishing) and the site's guide,
<https://paperpaw.org/authors>, say what to submit and how.

## Size

The sample's file is about 79 KB for the X4, built with
`opt-level = "s"`. With `opt-level = "z"` it gets smaller, and slower.

Most of it is the SDK code the plugin carries (drawing text, settings,
translations, `core`'s formatting); the sample's own code is some 9 KB.
