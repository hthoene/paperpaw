# Interface-7 plugin files (format 5)

PaperPaw's own plugins that have a native entry (`native-entry` feature of
the plugin crate, `paperpaw_sdk::native_plugin!`), each as a plugin file of
format 5 (`abi/ppn5.md`) for the X4: one `src/bin/<id>.rs` per plugin, built
with the linker script `paperpaw-plugin7.x` (the copy of
`crates/paperpaw-native/paperpaw-plugin7.x`) and packed by `paperpaw-sign pack`.

    tools/plugin-files7/build.sh x4 <out-dir> [--key <file>] [--only <id>]

`tools/flash-gate.sh` counts these files on the head side for the plugins that
ship as files. A plugin the packer refuses (a writable `.data`/`.bss`) is named
and the exit status is 1; the others are still built. The built-in plugins that
the firmware links (`paperpaw_app::NATIVE_PLUGINS`) are built here too, so
their size as a separate file is known; they are not shipped as files.

## The X4 Pro and the X4 Classic (ESP32-S3)

    tools/plugin-files7/build.sh x4-pro <out-dir>

builds every plugin for `xtensa-esp32s3-none-elf` with espup's `esp` toolchain
(`cargo +esp x4pro`, against the release's core libraries, `tools/xtensa-std`),
linked with the same script, and packs each into a format-5 file of target 9
(`abi/ppn5.md`, "Target 9", decision 142): `paperpaw-sign pack` has an Xtensa
branch, and takes the plugin's ABI metadata from
`plugins/<id>/native/metadata.s3.bin`, the generated record of the S3's profile
(`tools/plugin-files7/s3-metadata.py` writes it; `--check` fails if one is
missing or differs). A missing or stale record stops the script with exit 3,
never a pass. A target-9 file has no hot code and no patches. The heap word is
`global_asm!`, which rustc gates on Xtensa: `.cargo/config.toml` opens
`asm_experimental_arch` for the build (the `esp` compiler is a nightly). The X4
Classic runs the X4 Pro's code: its files are the Pro's bytes under the Classic's
name, and CI and the release build both devices.

`tools/plugin-files/catalogue.py` reads a format-5 file's header as `abi` =
`"7.<needs_minor>"` (bytes 8..10 and 10..12, decimal), checks the file's chip
against its device, and writes `path = "tools/plugin-files7"` as the source; a
format-4 file keeps its hex fingerprint and `tools/plugin-files`. One entry never
mixes the two.
