# M1 metadata emitter draft

This checkpoint describes **compatibility metadata**, not the PPNP5 image header,
relocations, signature envelope, platform preflight or a working plugin loader.
Its schema is `abi.def.metadata`; generated offsets and record widths are in
`generated/metadata-layout.json`. Consumers must use that descriptor rather than
copying offsets. The trusted lock includes the resolved primitive widths and
field offsets as well as the schema.

The encoding uses little-endian scalar fields. The prefix declares magic, layout
version, prefix and total byte counts, ABI major, required minor, entry layout and
size, zero flags, and target/service/feature/export lengths or counts. The body
contains the target identifier, exports, services and semantic features, in that
order. Every variable record has its own byte length and string length. Keys are
bounded UTF-8; service and feature requirements are ordered canonically and must
not be duplicated. No address, Rust layout fingerprint, compiler identity or
self-covering digest appears here. The external envelope will cover these bytes.

A build supplies `target`, `used`, `floor`, `services` and `features`. It cannot
supply a downward `needs_minor` override. Host entries, records and individual
fields contribute their declared minor; a field also requires its enclosing
record's minor. Selected services and semantic features contribute their minor
as well: the interface minor that introduced the service or feature, never a
service's own version (its `major` and `semantic_revision` in the service record,
see `docs/abi/proposals/interface-7-usage-clock.md`). Empty `used` requires the current SDK minor as a conservative floor.
This does not discover actual code usage: generated stubs and build provenance
must supply an honest complete dependency set before runtime qualification.

The service catalog holds the four data services of slot 17 (`core.usage`, `core.plugin-usage`,
`core.plugin`, `core.work-note`): major 1, semantic revision 0 (the service minor), no call table
(`min_table_bytes` 0; a service with a call table declares at least its 4-byte size word), since minor 1.
A selected service contributes the interface minor that introduced it, never its semantic revision.

Metadata is limited to 2,048 bytes, keys to 128 bytes, exports to one, services to
32 and features to 32.

A feature record carries `flags` (u16). Bit 0 means required (D1, #890): a host that
does not know a required feature key refuses the load and names the key, while an
unknown key without the bit is ignored. Other bits are refused. Known keys must
match the catalog revision. Service keys stay closed: an unknown one is refused. The codec rejects unknown identifiers, invalid lengths,
reserved fields, duplicates, invalid UTF-8, noncanonical requirements and trailing
bytes. A higher required minor is structurally valid; the loader must separately
refuse an unsupported requirement before invoking plugin code. These are host-tool
bounds, not measured X4 parser allocations or a runtime memory qualification.

The initial profile rows and entry metadata view are specified in
[entry.md](entry.md). Unknown/unregistered profiles still fail. The entry's static
metadata view is compared byte for byte before `make`; no compiler identity or
truncated target token substitutes for that comparison. Private service/extension
declarations need their own generated contract path before the follow-on custom
service slice; the current catalog is not a final restriction on third-party services.

The intended CLI is:

```
python3 tools/abi-metadata.py --schema abi/abi.json --input build.json \
  --bytes metadata.bin --json metadata.json
```

Both outputs are validated before either is opened. Output paths must be distinct
from each other and from inputs. Filesystem write failures can still leave only
one output; packaging must check command success and consume the pair from the
same successful invocation. No artifact is signed or made executable here.

The shared fixture manifest is `fixtures/metadata/manifest.json`. Its target is
explicitly synthetic and never executable; only the metadata verdict is tested.
Container, compatibility, signature and execution stages are `not_run`. Regenerate
with `python3 tools/abi-metadata-fixtures.py`; CI uses `--check`.

Remaining qualification: real-platform loader fixtures, generated Rust bounded reader, container placement and duplicate/mapping
checks, loaded-entry comparison, signature coverage, cross-compiler fixtures and
per-target runtime tests. Environment snapshot layout remains separate.

Metadata format 2 removes the redundant host-slot export from the inactive
format-1 draft. The trusted-lock transition is permitted only before any
format-1 target profile was published, retains the entry export and all other
encoding fields, and obeys the released minor floor. Current artifacts use only
format 2; the earlier schema is understood for trusted-lock verification.
