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
