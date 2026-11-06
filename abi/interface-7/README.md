# Plugin interface 7: the draft baseline

Interface 7 (ADR 0034) is raised early on `abi7/integration` as a **draft** (`native::DRAFT`, `draft` in
`crates/paperpaw-sdk/interface.txt`). Its reference files are the ordinary ones of ADR 0026:
`crates/paperpaw-sdk/interface.txt`, `crates/paperpaw-sdk/interface/` and each crossing crate's `api.txt` and
`shape.txt`, written by `tools/interface.py` and blessed with
`PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface`. The temporary partition that kept the
`paperpaw_native::host_glue` namespace apart from the frozen interface-6 files is retired with the raise: the
host table is part of `paperpaw-native.api`/`.shape` like any other item.

While it is a draft:

- **Additions** (a new type, module, function or constant; what plugins built before cannot rely on) are blessed
  at the same draft.
- **A change plugins rely on** (a signature, a field, a variant, an order, a layout) takes the next draft:
  `native::DRAFT` + 1. The draft is part of `native::FINGERPRINT`, so plugin files built for an earlier draft are
  refused, never loaded.
- **Every bless is named.** `blessings.txt` gains a line `<draft> <what changed> (#<ticket or PR>)` in the same PR;
  lines are only appended, and the check compares the file with the PR's base every time, also where nothing
  else changed. CI (`tools/abi-reference-check.py`, job "Stable ABI against trusted base") compares the
  reference files with the PR's base. **At the same draft only additions pass:** `interface.txt` stays
  byte-equal, every reference file of the base keeps all its lines in their order, and files or lines may be
  added, with a new line in `blessings.txt`. Anything else (a removed, changed or reordered line, another
  fingerprint) takes the next draft. The draft moves by exactly one (no skipping, never back), `interface.txt`
  states the same interface and draft as `native::INTERFACE` and `native::DRAFT`, and a line that names
  another draft than `native::DRAFT` is refused.
- **A raise** (interface N+1) is only allowed from a frozen interface N and lands at draft 1, never later and
  never straight at the freeze; its `blessings.txt` must not exist in the base yet.
- **Freezing** is the owner's decision. The freeze PR deletes `native::DRAFT` and the line `draft = N` of
  `interface.txt` without a trace (nothing stays behind as 0), and appends `0 frozen ... (#...)` here. `DRAFT`
  is hashed into the fingerprint only while it exists, so the frozen interface-7 fingerprint is byte for byte
  the one it would be without any draft concept, and it refuses every draft's builds. From then on the
  reference bytes stay as they are, and only a raise to interface 8 changes what plugins see.

The C boundary itself (`abi/abi.def`, `abi/abi.lock`, `abi/generated/`) keeps its own append-only check against
the trusted base's lock (`tools/abi-check.sh --ci`, the same job). `raise.md` records the rows the raise and the
re-blesses after it apply.
