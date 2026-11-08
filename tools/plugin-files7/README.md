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
linked with the same script. It does **not pack** yet: format 5 has no target-9
profile (`abi/ppn5.md`, decision 142), so no `.ppn` is written. Until
`plugins/<id>/native/metadata.s3.bin` (the generated ABI metadata of the S3's
profile) exists for every plugin the script exits 4, "pending", which CI shows
as a notice and never as a pass. The heap word is `global_asm!`, which rustc
gates on Xtensa: `.cargo/config.toml` opens `asm_experimental_arch` for the
build (the `esp` compiler is a nightly). The X4 Classic runs the X4 Pro's code,
so CI builds one.

`tools/plugin-files/catalogue.py` reads a format-5 file's header as `abi` =
`"7.<needs_minor>"` (bytes 8..10 and 10..12, decimal), checks the file's chip
against its device, and writes `path = "tools/plugin-files7"` as the source; a
format-4 file keeps its hex fingerprint and `tools/plugin-files`. One entry never
mixes the two.
