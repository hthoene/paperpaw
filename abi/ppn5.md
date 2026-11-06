# The X4 `.ppn` file for interface 7 (format 5): proposal

Status: **proposal for review** (assignment #787, ticket #228). Docs only: nothing here is
implemented, generated or blessed. It is the input that ADR 0034 section 10 asks for
("concrete byte offsets are frozen only with the first generated layout and fixture review").
Kernel owns the schema and generator that will emit the offsets (`abi.json`); Firmware owns the
reader and the X4 loader. Nobody hard-codes the numbers below once the generator emits them.

**Amends** ADR 0034 sections 6 and 10 (the `.ppn` header, which they describe by fields only)
and takes the place of ADR 0024's "The `.ppn` file" for interface 7. `PAPERPAW_HOST` of
section 10 is *not* kept: `abi/entry.md` (later) says the host table reaches the plugin only
as an argument of `make`, with no global slot and no writable static. Interface-6 files
(format 4, `crates/paperpaw-native/src/header.rs`) stay as they are until they are retired.

## Header (little-endian, 96 bytes)

| Bytes | Field | Rule |
|---|---|---|
| 0..4 | magic `PPNP` | else not a plugin |
| 4..6 | `format` = 5 | below: older PaperPaw's file; above: newer |
| 6..8 | `target` | chip id as today (5: ESP32-C3, the X4); 9 (X4 Pro) only once its profile exists in `abi/entry.md` |
| 8..10 | `abi_major` = 7 | |
| 10..12 | `needs_minor` | from the generated metadata; above the host's: needs a newer PaperPaw |
| 12..14 | `draft` | the interface-7 draft the file was built for, **while interface 7 is a draft**; 0 after the freeze (then required 0) |
| 14..16 | `flags` | 0; anything else is refused as a newer format |
| 16..20 | `base` | data address the image is linked at, as in format 4 |
| 20..24 | `image_len` | read-only data, then code |
| 24..28 | `code_at` | |
| 28..32 | `entry` | offset of `PAPERPAW_ENTRY`, 8-aligned, `entry_bytes` of the target profile (20 on the ESP32-C3) long |
| 32..36 | `meta_at` | offset of the metadata record (`PPAM`, format 2) in the read-only data |
| 36..40 | `meta_len` | = the record's `total_bytes`, at most 2,048 |
| 40..44 | `relocs` | as format 4 |
| 44..48 | `align` | 4..4096, power of two |
| 48..52 | `hot_at` | as format 4 (RISC-V only) |
| 52..56 | `hot_len` | |
| 56..60 | `patches` | |
| 60..64 | reserved | 0, else refused as a newer format |
| 64..96 | SHA-256 | of bytes 0..64 and everything after the header |

After the header, unchanged from format 4: the image, then `relocs` u32 offsets (increasing), then
`patches` pairs of u32, then the author's signature if signed (112 bytes).

**Removed from format 4:** the fingerprint (8..16), `api` (32..36, no host slot), the compiler (44..48),
`needs_api` (60..64), the eight family revisions (64..80) and `interface` (80..82). Nothing
about layouts, compilers or Rust types is in the file.

**The first 16 bytes never move** (magic, format, target, `abi_major`, `needs_minor`, `draft`,
`flags`) while the draft lasts. So a later draft may change anything after them, and an older
or newer draft is refused by `draft` before any other field is read.

## Identity: no fingerprint, a draft number

- `FINGERPRINT` is not used for a format-5 file. `Native::fits` / `Fingerprint::misfit` keep deciding
  interface-6 (format-4) files only, and those are refused by their format number first
  ("built for an older PaperPaw"), as ADR 0034 section 6 says. So a draft change at
  `native::DRAFT` (PR #186) cannot reach the format-5 decision either way.
- The flat C boundary is versioned by `abi_major` / `needs_minor` (ABI lock). During the draft that
  is not enough, since a draft may change what a minor means. Hence `draft`: the header and the
  host carry the same number; **a file of another draft is refused, never loaded** (older: "built
  for an earlier draft, rebuild"; newer: "needs a newer PaperPaw"). The packer writes
  `paperpaw_sdk::native::DRAFT` into the field and the loader compares it with the same
  constant. The ABI lock is append-only and has no draft field; it gets none.
- **The freeze leaves no trace (owner's condition on #186), so the field cannot stay as a
  required-zero.** A reserved-zero field *is* a trace of the draft concept in the frozen format.
  Decided (Manager, 2026-11-06 11:14; Senior may veto on technical grounds only): the draft header is **format 5** and is never a released format; the freeze PR
  publishes the frozen header **without `draft`** as **format 6** (later offsets move once, in the
  freeze batch, where the generator re-emits every layout anyway). A file built at a draft is then
  refused by its format number ("built for an older PaperPaw") before any offset is read, exactly
  like an interface-6 file; no draft file can be misread by the frozen loader. The freeze deletes
  `native::DRAFT` and with it the field; nothing of the draft stays in the host, the lock or the
  file. (Considered and not taken: keep format 5 with two required-zero reserved bytes, a layout
  fact that names nothing but would still be a trace.)

## The loader's order (X4; the same as ADR 0034 section 6)

1. Fewer than 96 bytes, wrong magic: damaged / not a plugin.
2. `format`: below 5 older, above 5 newer.
3. `target`: unknown or other chip: built for another chip.
4. `abi_major`: below 7 built for an older interface; above: needs a newer PaperPaw.
5. `draft`: not the host's: as above.
6. `needs_minor` above the host's minor: needs a newer PaperPaw.
7. `flags` / reserved nonzero: newer format. Parts hold together: the extents of format 4 plus
   `meta_at + meta_len <= code_at`, `meta_len <= 2,048`, `entry + entry_bytes <= code_at`. Else malformed.
8. Digest over header and file; signature; installed hash; placement (ADR 0024, unchanged).
9. The metadata record at `meta_at` is read with the generated bounded reader and must agree
   with the header (`abi_major`, `needs_minor`, target profile). After relocation and before
   `make`, the entry's static metadata view must equal the record byte for byte (`abi/entry.md`).
   A mismatch is damaged. No plugin code has run until here.

A refused file stays installed and switched off with its data.

## What #756's freestanding rule becomes on the X4

An X4 image has no dynamic linking: no `DT_NEEDED`, no imports, no loader-run initialisers. The
loader's first call into the plugin is `make`. So the rule splits in two:

- **At pack time** (`paperpaw-sign pack`, from the linked ELF it is given, which still has symbols):
  refuse an undefined symbol, any exported symbol besides `PAPERPAW_ENTRY` and
  `PAPERPAW_METADATA`, a writable section (`.data`, `.bss`, TLS), `.init*`/`.ctors`/`.fini*`, and a
  relocation kind outside the short list. The same names and list as `PLUGIN_EXPORTS`.
- **At load time**, which sees no symbols: only what the file still shows. The structural checks of
  step 7 and 9, the relocation words (increasing, inside the image, 4-aligned, naming words that
  hold an address) and patches (known kinds, inside the code), the entry's size against the target
  profile, and its `make` and metadata pointers landing in the image. Trust in the pack-time check
  is the signature plus the installed hash, as for every code plugin.

## Size caps

`image_len` at most 4 MiB (`MAX_IMAGE_LEN`, above the X4's room; the real limit is the flash room,
2,273,280 bytes for all plugins); `meta_len` at most 2,048; `relocs` at most `image_len / 4`;
`patches` at most half the code's words; `align` 4..4096; signature 112 bytes. All checked before
anything is read at the extents they name; counts multiply with checked arithmetic.

## Open for the reviewers

1. **`draft` in the header** (above) against keeping it only in the metadata record (smaller, but
   the layout has to be read first, and the metadata is generated from the lock, which has no draft).
2. **Hot code and patches stay** in format 5 (the X4's RISC-V speed), unchanged from format 4.
3. **X4 Pro (ESP32-S3):** no Xtensa profile exists in `abi/entry.md`; `target` 9 is reserved, nothing
   else is specified for it here.

## Slice 1 (after PR #186 has merged)

A host-testable reader in `paperpaw-native`: `Header5::read(bytes, host)` with the host's
`(target, abi_major, minor, draft)` and a refusal per step 1 to 7 (wrong magic, older/newer
format, other chip, older/newer interface, stale or future draft, needs newer minor, odd or
truncated header, parts that do not hold together, nonzero reserved). Unit tests for each, no
firmware change. Firmware wiring (slice 2) comes after this note is approved.
