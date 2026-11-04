# The ESP32-S3's core libraries for code plugins

Code plugins for the X4 Pro (`xtensa-esp32s3-none-elf`) link with `core`, `alloc` and `compiler_builtins`. rustup has no build of
them for this target, and espup's `esp` toolchain has their source only,
so plugins used to build them with `-Zbuild-std`. A build so is not the
same on every computer: Cargo hashes the libraries' folder in the
toolchain as the system writes it (`\` on Windows), the hash goes into
every crate's `-C metadata`, and the plugin's code changes with it. The
plugin library's rebuild, on Linux, then matches no file built on
Windows.

So each release builds them once, on Linux, and publishes them
(`paperpaw-xtensa-std-<version>.tar.gz`, signed like the emulators). Installed
into the `esp` toolchain, they are where rustc finds a target's
libraries, as rustup puts the X4's: plugins build without `-Zbuild-std`
(the alias `cargo +esp x4pro` of `samples/hello-native`). The X4 Pro is
then where the X4 is: a file built on Linux or macOS with the sample's
`build.sh` matches the rebuild. On Windows, two more things must hold,
as for the X4, and `build.sh` does not do them. First, every folder with
Rust files is remapped on its own. Else the paths below the top keep
their `\`. Second, Cargo is told the Linux host: a `rustc` that says
`host: x86_64-unknown-linux-gnu` in `-vV`. Else the build scripts'
hashes differ. With both, and the SDK from a registry, a Windows build and a
Linux build of the sample packed to the same file, byte for byte
(measured 2026-10-23, `esp` 1.97.0.0). The firmware still builds its own
with `-Zbuild-std`; the libraries installed do not change that.

| | What |
|---|---|
| `build.sh <out-dir>` | builds the libraries as plugins are built (`Cargo.toml`'s profile, their bitcode for link time optimisation, the toolchain's paths written as `/rust`) and packs them, the archive the same on every run |
| `install.sh <archive>` | installs them into the `esp` toolchain, those of another release removed first; refused where the toolchain is not the one they were built by |

A release's archive fits only the `esp` toolchain of that release
(`shells/xteink/versions.env`). To check by hand that it holds, or to
install it without this repository:

```sh
paperpaw-sign check-asset paperpaw-xtensa-std-0.6.0-beta.2.tar.gz paperpaw-xtensa-std-0.6.0-beta.2.tar.gz.sig
sysroot="$(rustc +esp --print sysroot)"
rm -rf "$sysroot/lib/rustlib/xtensa-esp32s3-none-elf"
tar -xzf paperpaw-xtensa-std-0.6.0-beta.2.tar.gz -C "$sysroot"
```

The signer is PaperPaw's release key, fingerprint 55A9 F626 DB17 EB90,
whole `55a9f626db17eb9058707323d29f52470bfcd09638898e17bd01784ef9df0c51`. The archive holds `core`, `alloc` and `compiler_builtins`,
all a plugin links with: CI builds the sample and PaperPaw's own store
plugins against them alone. CI also checks that plugin files are the
same in any folder (`tools/plugin-files/anywhere.sh`).
