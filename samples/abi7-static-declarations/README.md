# Static declaration batches

Run `cargo run --manifest-path samples/abi7-static-declarations/Cargo.toml`.
The build script runs the SDK declaration builder (and therefore its generated
encoders) on the host. It writes Rust `static` arrays into `OUT_DIR`; the
`no_std` library includes them in the plugin image. The bytes need no runtime
encoding or allocation; `register_widgets` uses the SDK registration facade.

The Battery and widgets fixtures use a local test-only conversion of their
current settings. A ported plugin should instead share its borrowed declaration
function between its build script and its runtime-builder equivalence test.
Do not add a dependency on the plugin itself to its own build script.

At `make`, iterate `DECLARATIONS` and submit each slice directly through the
registration SDK facade. Do not encode, clone or copy into a stack buffer:
Core's image-reference path depends on the pointer being inside the loaded
read-only image. Stop construction on the first returned error. The sample's
sink demonstrates this forwarding without claiming a live host integration.

The build fails if either encoded stream differs from its existing golden.
The host tests additionally check the compiled static bytes, unchanged slice
pointers, zero submission heap and first-error propagation. This measures only
plugin submission, not Core's registry indexes or device memory qualification.

Build the target library with `cargo build --release --lib --target
riscv32imc-unknown-none-elf --manifest-path samples/abi7-static-declarations/Cargo.toml`.
Inspect its object sections to verify the bytes reside in read-only data.
The desktop executable is a reporting harness, not a native plugin loader.

Run `python3 samples/abi7-static-declarations/check_rodata.py <desktop-binary>
<riscv32-rlib>` to verify all 51 batch symbols (61,815 bytes) are in read-only
`.rodata`. Widgets accounts for 50 batches and 61,585 bytes. Host submission
tests measure zero peak and resident heap; Core registration costs are separate.
