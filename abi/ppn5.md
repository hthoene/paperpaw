# The X4 `.ppn` file for interface 7 (format 5): proposal

Status: **proposal for review; the draft question is decided (below)** (assignment #787, ticket #228). Docs only: nothing here is
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
| 6..8 | `target` | chip id as today (5: ESP32-C3, the X4); 9 (X4 Pro) stays refused until an Xtensa profile exists in `abi/entry.md` |
| 8..10 | `abi_major` = 7 | |
| 10..12 | `needs_minor` | from the generated metadata; above the host's: needs a newer PaperPaw |
| 12..16 | `flags` | 0; anything else is refused as a newer format |
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
`patches` pairs of u32, then the author's signature if signed (112 bytes). The file is exactly
`96 + image_len + 4 * relocs + 8 * patches` bytes, plus 112 if signed; any other length is refused.

**Removed from format 4:** the fingerprint (8..16), `api` (32..36, no host slot), the compiler (44..48),
`needs_api` (60..64), the eight family revisions (64..80) and `interface` (80..82). Nothing
about layouts, compilers or Rust types is in the file.

**Format 5 is the frozen layout from the day it is written.** The first 16 bytes (magic, format,
target, `abi_major`, `needs_minor`, `flags`) are read first, and a refusal by any of them comes
before any other field is read.

## Identity: no fingerprint, no draft number

- `FINGERPRINT` is not used for a format-5 file. `Native::fits` / `Fingerprint::misfit` keep
  deciding interface-6 (format-4) files only, and those are refused by their format number first
  ("built for an older PaperPaw"), as ADR 0034 section 6 says. So `native::DRAFT` (PR #186) cannot
  reach the format-5 decision either way.
- **There is no draft field in the file.** The C boundary is append-only against the trusted lock
  during the draft as well as after it, and the major-7 lock is never re-baselined, so a number in
  the header would add no protection: a plugin built at any point of the draft is a prefix of what
  the host knows. The real guard is elsewhere: the plugin's `make` checks `host.size` before it
  touches a host entry (made mandatory in the generated/SDK glue, Kernel's), and the host refuses
  unknown record tags. The header says what must be true before any code runs: `abi_major`,
  `needs_minor` and the target.

## The loader's order (X4; the same as ADR 0034 section 6)

1. Fewer than 96 bytes, wrong magic: damaged / not a plugin.
2. `format`: below 5 older, above 5 newer.
3. `target`: unknown or other chip: built for another chip; 9 refused.
4. `abi_major`: below 7 built for an older interface; above: needs a newer PaperPaw.
5. `needs_minor` above the host's minor: needs a newer PaperPaw.
6. `flags` / reserved nonzero: newer format.
7. Exact checks of the header and the file, all before anything is read at an extent a field names
   (arithmetic checked, no overflow):
   - the file length is exactly the formula above;
   - `code_at <= image_len`, and `code_at`, `meta_at` and `base` are aligned (`code_at` and `meta_at`
     to 4, `base` to `align`); `align` is a power of two in 4..4096;
   - `meta_at + meta_len <= code_at`, `meta_len <= 2,048`; `entry + entry_bytes <= code_at`;
   - `relocs <= image_len / 4`, `patches` at most half the code's words, hot code as format 4.
   Else malformed.
8. Digest over header and file; signature; installed hash; placement (ADR 0024, unchanged).
9. Before relocation, every relocation word lies inside `[base, base + image_len)`. The metadata
   record at `meta_at` is read with the generated bounded reader and must agree with the header
   (`abi_major`, `needs_minor`, target profile). After relocation and before `make`:
   - `entry.metadata.ptr == base + meta_at` and `entry.metadata.len == meta_len`, and the view is
     equal to the record byte for byte (`abi/entry.md`);
   - `entry.size` equals the entry length of the target profile, and the entry's `abi_major` and
     `needs_minor` equal the header's;
   - `make` lies in `[code_at, image_len)` and is 2-byte aligned (RISC-V compressed instructions).
   A mismatch is damaged. No plugin code has run until here.

A refused file stays installed and switched off with its data.

## What #756's freestanding rule becomes on the X4

An X4 image has no dynamic linking: no `DT_NEEDED`, no imports, no loader-run initialisers. The
loader's first call into the plugin is `make`. So the rule splits in two:

- **At pack time** (`paperpaw-sign pack`, from the linked ELF it is given, which still has symbols):
  refuse an undefined symbol, any exported symbol besides `PAPERPAW_ENTRY` and
  `PAPERPAW_METADATA`, a writable section (`.data`, `.bss`, TLS), `.init*`/`.ctors`/`.fini*`, and a
  relocation kind outside the short list. The same names and list as `PLUGIN_EXPORTS`.
- **At load time**, which sees no symbols: only what the file still shows, that is step 7 and 9
  above, the relocation words (increasing, 4-aligned, inside the image, naming words that hold an
  address) and the patches (known kinds, inside the code). Trust in the pack-time check is the
  signature plus the installed hash, as for every code plugin.

## Size caps and memory

`image_len` at most 4 MiB (`MAX_IMAGE_LEN`, above the X4's room; the real limit is the flash room,
2,273,280 bytes for all plugins); `meta_len` at most 2,048; `relocs` at most `image_len / 4`;
`patches` at most half the code's words; `align` 4..4096; signature 112 bytes; `hot_len` at most the
X4's IRAM arena (`HOT_MAX`, 16 KiB, as in format 4). The digest, the relocation words and the patches are
**streamed through a fixed buffer**: nothing is allocated in proportion to the file, the image,
`relocs` or `patches`, on the 256 KiB heap or elsewhere.

## Decisions (Manager, 2026-11-06, on Senior's review #792)

1. No `draft` field and no format 6 at the freeze: see "Identity".
2. **Hot code and patches stay** as in format 4 (the X4's RISC-V speed).
3. **X4 Pro (ESP32-S3):** target 9 stays refused until an Xtensa profile exists in `abi/entry.md`;
   nothing else is specified for it here.

## Slice 1

A host-testable reader in `paperpaw-native`: `Header5::read(bytes, host)` with the host's
`(target, abi_major, minor)` and a refusal for each check of steps 1 to 7 (wrong magic, older/newer
format, other chip, target 9, older/newer interface, needs newer minor, odd or truncated header,
nonzero flags or reserved, wrong file length, misaligned or overlapping parts, oversized counts),
and for the entry checks of step 9 as a function over the relocated bytes. Unit tests for each; no
firmware change. It does not depend on PR #186. Firmware wiring (slice 2) follows once this note
is merged.
