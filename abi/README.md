# Interface 7 stream A: draft grammar checkpoint

This is an experimental M1 schema, not a frozen ABI or a working plugin path.
`abi.def` uses strict JSON: duplicate/unknown keys, nonfinite numbers and booleans
where integers are required are rejected. JSON object ordering is immaterial;
array ordering is ABI ordering. IDs are explicit and cannot be reused.

The root has `format`, `major`, `minor`, `host_entries` and `records`. Entries
have `id`, `name`, `since`, ordered `args`, `result`, `effect`, `ownership`,
`reentry` and `max_bytes`. Names are lower-case dotted identifiers. Arguments
have `name` and `type`; the closed primitive vocabulary is in `abi-gen.py`.
Results are `status`, `length` or `count`; only repeatable queries use `length`.
`count` is bytes actually copied, never greater than output capacity; zero means
EOF for a positive-capacity blob read (a zero-capacity request copies nothing).
`blob.read` is positional and returns `count`; it is not a snprintf sizing query.
Effects are `query`, `consume`, `mutate`; ownership is `call`, `owner`, `release`.
This first checkpoint accepts only `deny` reentry. Buffer entries need a positive
cap. Entry caps are schema maxima, not promises of allocation or admission.

Records have `id`, `name`, `since` and ordered `fields`. Fields have `name`,
`type`, `since`, `optional`, `ordinal`, `default`, `max_bytes`. Current wire types
are u32/u64/i32/str/bytes. Required fields have null ordinal/default. Optional
ordinals are unique 0..255 and defaults must fit their type. An existing record
may gain only optional fields with defaults in the next minor. Changing required
interpretation needs a new record. Record encoding is not implemented here.

Commands:

```
python3 tools/abi-gen.py --check
python3 -m unittest discover -s tools/tests -p test_abi_gen.py
python3 tools/abi-gen.py --baseline /path/to/trusted/prior/abi.lock --check
```

Normal generation writes abi.json, generated/paperpaw-abi.h and
generated/paperpaw_abi.rs, never the lock. The raw Rust module can be included
by path without workspace membership. It uses core only; no root manifest edit
is needed. Run `bash tools/abi-check.sh` for schema, native C/Rust size/alignment/
field-offset agreement and host/X4 no_std compilation checks.
`--write-lock` explicitly writes the lock after validating compatibility with the
old one. The lock contains the schema, released-minor boundary and exact emitted
C and Rust declarations. Re-rendering the trusted OLD schema with the current generator
must reproduce those declarations, catching changed primitive/view/prefix/result
templates even when a candidate lock was regenerated. Whitespace changes to the
emitter are conservatively reviewed too. Existing signatures/contracts stay
immutable. Multiple additions may use the same unreleased minor; additions to a
released minor fail. Each record field list is checked separately.
Use `bash tools/abi-check.sh --ci` with mandatory `ABI_BASELINE` in CI.
CI must compare with a trusted integration/release lock as well as generated
freshness; editing both definition and local lock is not compatibility evidence.
Until a minor is released (interface 7 until 1.0) only the lock matching the code
is checked: a PR that changes the ABI carries its regenerated lock (`--write-lock`).

The C header and Rust module declare the same first-27 host table. Both remain
raw table skeletons. Never create a full `&PpHost` from a shorter table; checked
prefix/slot wrappers are a separate pending step. It does not implement safe
slot access, lifetime wrappers, host dispatch, schema-derived semantic errors,
callback tables, named value types, wire codecs, resource accounting, complete
artifact layouts or a runtime. The separate metadata emitter draft is described
in [metadata.md](metadata.md), with entry/locator and initial target layout
declarations in [entry.md](entry.md). Runtime integration remains unqualified. Those remain stream A/M1 work. No loader
may infer binary metadata offsets from this checkpoint. Compiler-independent
calling convention and layout need per-target tests beyond the host syntax check.

The independently reviewed mutation matrix includes callback and nested-record
cases not supported yet; they must be added with those grammar features before
approval. Existing production interface-6 code is untouched.

The initial schema-only lock was replaced explicitly while still an unpublished
draft. The complete C-only draft lock (format 1) can be explicitly extended to format 2
with Rust declarations, after its C contract and release floor pass unchanged.
The older schema-only draft lock is still refused. This is not evidence of
compatibility with that former incomplete lock. Shared nested input/output
capacity and runtime quota enforcement remain M1 obligations.

The candidate released-minor boundary cannot be lower than the trusted baseline
in any mode. Initial lock creation with a baseline inherits that floor, preserving
it when the candidate becomes the next integration baseline.

The `statuses` declarations give each status a name, signed i32 value and `since`
minor. `OK` is zero; named errors are negative; unknown negative results remain
failures. Existing values, names and meanings cannot change. This draft extension
also emits `MAJOR`/`MINOR` in Rust (`PP_ABI_MAJOR`/`PP_ABI_MINOR` in C), derived
from the schema, not duplicated in shell glue. Status constants are `OK` and
`ERR_*` in Rust and `PP_OK`/`PP_ERR_*` in C. A prior draft without statuses retains
its exact declarations when checking that prior lock; current outputs gain the
new constants explicitly. Host field layouts and function signatures do not change.

Metadata schema/offsets are included in lock format 3. Draft lock formats 1/2
can be explicitly extended while preserving their emitted C/Rust contracts and
release floor. `bash tools/abi-check.sh` includes metadata/requirement tests and
freshness of the metadata-only synthetic fixture manifest.

The generator also writes `crates/paperpaw-native/src/host_glue/paperpaw_abi.rs`
for standalone Cargo packages. `--check` requires this copy to match the same
Rust output as `abi/generated/paperpaw_abi.rs`; do not maintain it by hand.
