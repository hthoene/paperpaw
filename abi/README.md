# Interface 7 stream A: draft grammar checkpoint

This is an experimental M1 schema, not a frozen ABI or a working plugin path.
`abi.def` uses strict JSON: duplicate/unknown keys, nonfinite numbers and booleans
where integers are required are rejected. JSON object ordering is immaterial;
array ordering is ABI ordering. IDs are explicit and cannot be reused.

The root has `format`, `major`, `minor`, `host_entries` and `records`. Entries
have `id`, `name`, `since`, ordered `args`, `result`, `effect`, `ownership`,
`reentry` and `max_bytes`. Names are lower-case dotted identifiers. Arguments
have `name` and `type`; the closed primitive vocabulary is in `abi-gen.py`.
Results are `status` or `length`; only repeatable queries may use `length`.
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

Normal generation writes abi.json and generated/paperpaw-abi.h, never the lock.
`--write-lock` explicitly writes the lock after validating compatibility with the
old one. Existing signatures/contracts are immutable; minor advances by at most
one, additions use the new minor, and each record field list is checked separately.
CI must compare with a trusted integration/release lock as well as generated
freshness; editing both definition and local lock is not compatibility evidence.
The initial draft may require reviewed baseline replacement before freezing.

The C header is a syntax-checkable table skeleton. It does not implement safe
slot access, lifetime wrappers, host dispatch, schema-derived semantic errors,
callback tables, named value types, wire codecs, resource accounting, artifact
layout/metadata emission or a runtime. Those remain stream A/M1 work. No loader
may infer binary metadata offsets from this checkpoint. Compiler-independent
calling convention and layout need per-target tests beyond the host syntax check.

The independently reviewed mutation matrix includes callback and nested-record
cases not supported yet; they must be added with those grammar features before
approval. Existing production interface-6 code is untouched.
