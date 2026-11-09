# Interface-7 `core.registry` stack (RISC-V)

The host side of `core.registry` (`crates/paperpaw-core/src/native_host/registry_service/`)
answers on the calling plugin's stack, so its frames are held to the X4's 2,048-byte
frame gate (`shells/xteink/frame-budget-ppn5.txt`, `each 2047 paperpaw_core::native_host`).
This entry records the chains read from the source; it is not a measurement. The
measurement with `tools/stack-frames.py` on a `riscv32imc` release build, as in
`docs/measurements/abi7-menu-stack.md`, is open (X4 timing and stack gate).

| Path | Stack held | Note |
| --- | ---: | --- |
| list op, row building (`reg_plugin`, `reg_item`, ...) | a few hundred bytes | `Fixed<N>` strings of at most 128 bytes, no heap |
| `reg_setting` (op 1, 2) with `Encoded<64>` | about 0.2 KiB | the value cap of the row |
| `reg_value` (op 18) with `Encoded<512>` | about 1.6 KiB | `bytes: [u8; 512]` + the encoder's `[MaybeUninit<u8>; 512]` + the returned value; the deepest chain of the service |

The `Encoded<512>` chain (preset and placement values) is the largest and stays under the gate
by itself; the visitor frames above it (`list`, the page builder) are small. Nothing here
allocates in the reply path: the 256 KiB heap is untouched. A change that deepens the chain
(a larger value cap, a second buffer) needs a new measurement.
