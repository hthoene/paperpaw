# PaperPaw's changes to hadris (exFAT, ADR 0034)

`patches/hadris-fat` and `patches/hadris-fs` are crates.io's hadris-fat and
hadris-fs **3.0.0-rc.2** (MIT), with the changes below. Each one is marked
"Modified for PaperPaw" where it is made. The other hadris crates
(hadris-fat-raw, -io, -storage, -common, -macros) are used as published.

`patches/hadris-diff.sh` prints all of them as one diff against the
release they came from. To move to a newer hadris:

1. `patches/hadris-diff.sh > /tmp/hadris.patch` (on the old release).
2. Unpack the new release over `patches/hadris-fat` and `patches/hadris-fs`
   (without its `tests`, `examples` and `benches`), and set the version in
   `crates/paperpaw-fat/Cargo.toml` and the root `Cargo.toml`.
3. `patch -p1 -d patches < /tmp/hadris.patch`, then check each hunk against
   the list below: drop the ones the new release has taken over.
4. Run `cargo test -p paperpaw-fat`, the exFAT fuzz target and an X4 build.

| # | Where | Change | Why | Feature |
|---|---|---|---|---|
| 1 | `hadris-fs/Cargo.toml` | `async-lock` and `spin` only where `target_has_atomic = "ptr"` | they need atomic pointers; the ESP32-C3 has none | always |
| 2 | `hadris-fs/src/tree.rs`, `api/tree.rs`, `api/read_tree.rs`, `api/volume.rs` | `Rc` used as `Arc` where pointers have no atomics | builds on the ESP32-C3 (one core, never shared) | always |
| 3 | `hadris-fat/src/exfat/fs.rs` | `valid_set` split into `shaped_set` and the checksum | the sweep (5) needs the shape alone; same result for reads | always |
| 4 | same, `unlink_entry` and `remove_entry_with(…, free)` | an entry set taken away without freeing its clusters, a folder's too, whatever it holds | a rename cut off by a power loss leaves two sets naming the same clusters: the stale one goes, the clusters stay | always |
| 5 | same, `sweep`, `plausible_stream`, `reseal`, `Swept` | what a power cut left in a folder's entries at an earlier mount is settled: a torn set whose stream is possible on the card is sealed again, a lone secondary entry is marked unused (a benign primary entry's own are skipped), anything else is left and counted | recovery across mounts, not only within one | always |
| 6 | `hadris-fat/src/block_io.rs` | `ZEROS` 4 KiB, not 64 KiB | 60 KB less flash | always |
| 7 | same | `ZEROS` 512 bytes; larger device blocks fill through the block buffer | 3.5 KB less flash | `small` |
| 8 | `hadris-fat/src/table.rs` | the node table is a vector sorted by id, not a `BTreeMap` (same interface) | about 5 KB less flash; a driver pins a few nodes at a time | `small` |
| 9 | `hadris-fat/Cargo.toml` | the feature `small`; `[[test]]`, `[[example]]` and `[[bench]]` sections removed | 7 and 8; the tests and examples are not vendored | — |

PaperPaw turns `small` on (`crates/paperpaw-fat/Cargo.toml`). Changes 1, 2,
4, 7 and 8 are proposed upstream (paperpaw-notes
`paperpaw/designs/hadris-upstream.md`); 3 and 5 belong to PaperPaw's way of
settling after a power cut and are offered as an idea only.
