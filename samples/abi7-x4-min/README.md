# abi7-x4-min: the smallest interface-7 plugin for the X4

An entry (`PAPERPAW_ENTRY`, 20 bytes on the ESP32-C3), the metadata record
the generator wrote (`PAPERPAW_METADATA`), and a plugin that does nothing: no
state, no events, a `drop` that frees nothing. One function (`table_ok`) is
hot code, called from `make`, so the file has patches. Plugin-owned code on
the published raw contract (`abi/entry.md`), with no facade; it calls no host
function and is not loaded by any firmware yet.

It is what `paperpaw-sign pack` is tried on (format 5, `abi/ppn5.md`):

```sh
export CARGO_TARGET_DIR=$PWD/target-abi7-x4-min     # an own one
samples/abi7-x4-min/build.sh --check     # builds, packs, compares with the committed file
samples/abi7-x4-min/build.sh --update    # writes the linked ELF and the file
```

Built with `cargo x4` (the Rust of `rust-toolchain.toml`, linked with
`paperpaw-plugin7.x`, a copy of the crate's; the medium code model, `--emit-relocs`
and `--no-relax`, as `.cargo/config.toml` says), then
`paperpaw-sign pack target/.../abi7_x4_min abi7-x4-min.ppn`. The file holds no path
and no symbol name, so it is the same wherever the sample is built. The two
files the tests read are in `crates/paperpaw-native/tests/data/`.

`metadata.bin` is what `tools/abi-metadata.py` writes for `metadata.json`
(`build.sh` checks it); no build script runs, as in a plugin's own repository.
