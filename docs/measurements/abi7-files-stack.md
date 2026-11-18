# Interface-7 `native::files` stack (RISC-V)

Frames of the typed `core.files` facade (`crates/paperpaw-sdk/src/native/files.rs`),
read from a release build for `riscv32imc-unknown-none-elf` with
`tools/stack-frames.py`: Rust 1.99.0, opt-level `s`, one codegen unit, no LTO,
panic abort. The compile-only probe is `tools/stack-probes/files.rs` (one
`#[inline(never)]` function per call, refusing allocator stubs; it is not run);
build it as `docs/measurements/abi7-menu-stack.md` describes for the menu probe,
and run `python3 tools/stack-frames.py ELF --from 'files_.*_probe' --paths 7`.
Static direct-call chains only: indirect calls and interrupt frames are not counted.

Measured at the head of the facade after the Kernel ruling on `remove_tree`.

| Call | Own frame | Deepest static chain |
| --- | ---: | ---: |
| `list` | 384 B | 1,248 B |
| `stat` | 352 B | 1,312 B |
| `space` | 224 B | 1,168 B |
| `remove_step` | 240 B | 1,184 B |
| `remove_tree` (adds a 96 B loop frame) | 96 B | 1,264 B |
| `move_entry` | 128 B | 512 B |
| `make_dir` | 128 B | 496 B |

Everything is under the X4's 2,048-byte frame gate. The deep chains are the reply
decoder, not the facade: `record_decode` (224 B) into the generated
`ServicePluginUsage::decode` (336 B) is the part of the chain that is not the
facade's own frame; the typed codecs of #364 would shorten it. No call allocates,
so the 256 KiB heap is untouched; the caller's buffers are 1,536 B in and 4,096 B
out (`INPUT_BYTES`, `OUTPUT_BYTES`), which the plugin owns (static or one frame
above, not both). X4 runtime measurement stays open.
