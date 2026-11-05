# Separate interface-7 contract gate

`paperpaw-native.api` and `paperpaw-native.shape` snapshot the inactive
`paperpaw_native::host_glue` host implementation, including the generated table
module and its public reexports. These lines are checked exactly, not ignored.
The namespace is not an interface-6 contract: the existing plugin macro and
active loader use `paperpaw_native::{Api, Entry}`, the SDK's interface-6 layout
and fingerprint, and the existing shared tables. Those inputs remain unchanged.

PR #142 publicly reexports `PpEntry` and `PpPlugin` for the separate inactive
`native_runtime`. Privacy is therefore not the reason for this partition.
`tools/interface.py` audits rustdoc references from public legacy declarations,
following private aliases and fields and checking the source ownership of
renamed reexports. A legacy export reaching this namespace is refused. The SDK
interface test also rejects a reference in a legacy API/shape line, including
an impl whose self type is outside the partition. No production loader is
switched by the gate change.

The original interface-6 comparator still checks every other API/shape item,
its layout fingerprint, toolchain pins and linker. Historical interface-7 lines
that were accidentally included there are filtered out of both sides. A normal
legacy bless can remove those misclassified lines but cannot alter the
interface-7 snapshots. Regression tests keep changed `Api`/`Entry` layouts
breaking even when the interface-7 snapshot matches.

To refresh the interface-7 snapshots after review, use:

```
ABI_BASELINE=/independent/trusted/abi.lock python3 tools/interface.py --write-abi7-baseline
```

This checks the trusted ABI lock before writing. Ordinary interface generation
and `PAPERPAW_BLESS=1` never bless the separate snapshot. `--check` and the SDK
interface tests require it to match even during legacy blessing. The ABI lock
continues to guard emitted C/Rust layouts, callbacks, minor floors and metadata;
CI must supply its baseline from the trusted PR base, never the candidate tree.
This snapshot does not replace that compatibility check or qualify runtime
activation, calling conventions, callback dispatch or resource limits.

The namespace partition is a temporary development gate for the owner's single
interface-7 batch, not a legacy backend or runtime compatibility shim. It must
be reassessed with the coordinated interface raise and retirement of the old
boundary. Any new namespace or permitted cross-contract dependency requires
explicit review, not a wildcard exception.
