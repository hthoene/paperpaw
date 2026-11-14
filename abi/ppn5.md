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
| 6..8 | `target` | chip id as today: 5 ESP32-C3 (the X4, X3), 9 ESP32-S3 (the X4 Pro, X4 Classic; profile `esp32s3-xtensa-lx7-ilp32-windowed-le` in `abi/entry.md`, draft 29, decision 142) |
| 8..10 | `abi_major` = 7 | |
| 10..12 | `needs_minor` | from the generated metadata; above the host's: needs a newer PaperPaw |
| 12..16 | `flags` | bit 0 `HEAP_WORD` (the plugin has `PAPERPAW_HEAP`, `abi/heap-word.md`); any other bit is refused as a newer format |
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
| 60..64 | `heap_at` | image offset of `PAPERPAW_HEAP` with bit 0 of `flags`; else 0, and nonzero is refused as a newer format |
| 64..96 | SHA-256 | of bytes 0..64 and everything after the header |

After the header: the image, then `relocs` u32 offsets (increasing, as in format 4), then `patches`
**patch records of three u32 each (12 bytes, `PATCH_BYTES`)**, then the author's signature if signed (112
bytes). The file is exactly `96 + image_len + 4 * relocs + 12 * patches` bytes, plus 112 if signed; any other
length is refused.

A patch record is `kind | site`, `from`, `to` (little-endian words). `kind | site` is as in format 4
(`site | kind << 28`, kind 1 `Hi20`, 2 `Lo12I`, 3 `Lo12S`, the site an image offset below 2^22). The loader
sets the bits of the instruction at `site` for the distance `runs_at(to) - runs_at(from)`, where `runs_at` is
the address an image offset is reached at once placed (hot code in the arena included). For `Hi20` (an
`auipc`) `from` is the site itself; for a lower part it is the site of its `auipc`, and `to` is where that
`auipc` reaches, so **a record holds everything its patch needs and nothing is looked up in another
record**. Records are **sorted by site, strictly increasing** (not, as in format 4, by kind and then site).
A record is valid when its kind is known, `site` is an even offset in the code with four bytes inside the
image, `to` is inside the image, and `from` is `site` (`Hi20`) or an even offset in the code with four bytes
inside the image (lower parts). Whether a lower part's `from` is an `auipc` that a `Hi20` record patches to the
same `to` is **not checked by the loader**: `paperpaw-sign pack` guarantees it, and the signature covers the
result like every other byte of the code.

**Removed from format 4:** the fingerprint (8..16), `api` (32..36, no host slot), the compiler (44..48),
`needs_api` (60..64), the eight family revisions (64..80) and `interface` (80..82). Nothing
about layouts, compilers or Rust types is in the file.

**Revision before any file existed (2026-11-06, Manager decision, see "Decisions" 4 and 5):** the patch
record grew from two words to three and the patches are sorted by site. No format-5 file had been written by any
packer yet, so there is no format 5 before this revision to keep; the number stays 5 and there is no format 6.

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
6. `flags` with a bit other than 0, or `heap_at` nonzero without bit 0: newer format.
7. Exact checks of the header and the file, all before anything is read at an extent a field names
   (arithmetic checked, no overflow):
   - the file length is exactly the formula above;
   - `code_at <= image_len`, and `code_at`, `meta_at` and `base` are aligned (`code_at` and `meta_at`
     to 4, `base` to `align`); `align` is a power of two in 4..4096;
   - `meta_at + meta_len <= code_at`, `meta_len <= 2,048`; `entry + entry_bytes <= code_at`;
   - with `HEAP_WORD`: `heap_at` is 4-aligned, `heap_at + 4 <= code_at`, and the word lies outside
     `[entry, entry + entry_bytes)` and `[meta_at, meta_at + meta_len)`;
   - `relocs <= image_len / 4`, `patches` at most the code's 2-byte instruction slots, `(image_len − code_at) / 2`, hot code as
     format 4.
   Else malformed.
8. Digest over header and file; signature; installed hash; placement (ADR 0024, unchanged).
9. Before relocation, every relocation word (the value in the image at a listed offset) is an
   address in the image by either bus view, as the relocation itself requires: `[base, base +
   image_len]` for data or `[base + CODE_OFFSET, base + CODE_OFFSET + image_len]` for code, the end
   included (a slice may point there). Function pointers such as `make` are code addresses. The metadata
   record at `meta_at` is read with the generated bounded reader and must agree with the header
   (`abi_major`, `needs_minor`, target profile). After relocation and before `make`:
   - `entry.metadata.ptr == placed_base + meta_at` (the base the image was placed and relocated to,
     `placement.base`, not the linked `base`) and `entry.metadata.len == meta_len`, and the view is
     equal to the record byte for byte (`abi/entry.md`);
   - `entry.size` equals the entry length of the target profile, and the entry's `abi_major` and
     `needs_minor` equal the header's;
   - `make`, as an offset into the image, lies in `[code_at, image_len)` and is 2-byte aligned (RISC-V
     compressed instructions). A `make` in hot code is mapped back to its image offset first
     (`hot_at + (make - placement.hot)`).
   - with `HEAP_WORD`: no relocation word lies at `heap_at`, and the word holds 0 in the file; the
     loader then writes the `PpHeap` address into the placed image (never into the digest's bytes).
   A mismatch is damaged. No plugin code has run until here.

A refused file stays installed and switched off with its data.

## What #756's freestanding rule becomes on the X4

An X4 image has no dynamic linking: no `DT_NEEDED`, no imports, no loader-run initialisers. The
loader's first call into the plugin is `make`. So the rule splits in two:

- **At pack time** (`paperpaw-sign pack`, from the linked ELF it is given, which still has symbols):
  refuse an undefined symbol, any exported symbol besides `PAPERPAW_ENTRY`,
  `PAPERPAW_METADATA` and the optional `PAPERPAW_HEAP` (`abi/heap-word.md`), a writable section (`.data`, `.bss`, TLS), `.init*`/`.ctors`/`.fini*`, and a
  relocation kind outside the short list. The same names and list as `PLUGIN_EXPORTS`.
- **At load time**, which sees no symbols: only what the file still shows, that is step 7 and 9
  above, the relocation words (increasing, 4-aligned, inside the image, naming words that hold an
  address) and the patches (known kinds, increasing sites inside the code, both places inside the image). Trust in the pack-time check is the
  signature plus the installed hash, as for every code plugin.

## Size caps and memory

`image_len` at most 4 MiB (`MAX_IMAGE_LEN`, above the X4's room; the real limit is the flash room,
2,273,280 bytes for all plugins); `meta_len` at most 2,048; `relocs` at most `image_len / 4`;
`patches` at most the code's 2-byte instruction slots, `(image_len − code_at) / 2`, **and at most 8,192
(`MAX_PATCHES`)**: every call from cold code into the hot code adds a record, however few call sites there are, and
the loader reads every record, so the count bounds its work; `paperpaw-sign pack` must refuse a file above it, so pack
and load agree; `align` 4..4096; signature 112 bytes; `hot_len` at most the
X4's IRAM arena (`HOT_MAX`, 16 KiB, as in format 4). The digest, the relocation words and the patches are
**streamed through a fixed buffer**: nothing is allocated in proportion to the file, the image,
`relocs` or `patches`, on the 256 KiB heap or elsewhere.

**The relocation reads both lists in order, once.** A piece of the image takes the next words and the next
patches whose sites lie in it, from the lists in the file, in runs of at most 16 words or 8 records through a
fixed buffer; a patch whose instruction reaches into the next piece is carried over. Nothing is looked up
in the file at a random place by the write path (a lookup, a binary search, is used only to start in the
middle: `Relocator5::apply` for the entry check, which touches words before `code_at` only). The X4 serves
the small reads from one block of the file held in RAM (512 bytes, `Cached`); how many reads that saves,
and what an install costs in time, is **to be measured on the device**.

**Every byte that decides what lands in the flash is covered by a digest compared before the plugin is
kept.** The write pass (`Relocator5::write_image`, which `verify` is, with nothing written) hashes each
piece of the image as it reads it for the flash, relocates it, and hands it to the writer. The words and the
patches it consumes are hashed as it reads them, in separate digests (one of all words, one of all
patches). After the image it reads the file's own words and patches once more, in file order, and hashes
them into the header's digest and into two digests of their own. Then three things must hold, else
`Damaged` and the caller takes back the plugin (`rewind`): the header's digest, the digest of the words
the relocation consumed equals that of the words in the file, and the same for the patches. A source that
answers otherwise the second time (a swapped card, a flaky read), whether in the image, in the lists, or
only to the relocation's own reads, cannot make the loader write a byte that the signed file does not
decide. The one thing a two-faced source can still do is make a refusal or an acceptance of the
**entry check** differ (it reads the words before `code_at` by lookup); it can not change what is written.

**A refusal costs no write.** Everything that can refuse a file is decided on the file in RAM before the flash
is touched: the header, the digest, every word to relocate, and the entry and the metadata record as they
will lie once placed. A file refused there is taken out of the layout as if it had never been placed, so the
plugins after it and the index are not written again for its sake, at this start or any later one. (Whether the
plugin manager also remembers the refusal, so the file is not read and hashed again at each start, is the
core's, not this note's.) Only a flash fault after the first erase rewinds the layout, and then the index is
written anew.

## Decisions (Manager, 2026-11-06, on Senior's review #792)

1. No `draft` field and no format 6 at the freeze: see "Identity".
2. **Hot code and patches stay** (the X4's RISC-V speed); the patch *record* changed, see 4.
3. **X4 Pro (ESP32-S3):** target 9 stays refused until an Xtensa profile exists in `abi/entry.md`;
   nothing else is specified for it here. *Superseded by decision 142 (2026-11-08, draft 29): see
   "Target 9" below.*
4. **Patch records are self-contained and sorted by site** (Manager, 2026-11-06, on Senior's #806 and the
   Manager's requirement that the signature decide every byte that is written). Changed from format 4: two
   words become three (`kind | site`, `from`, `to`), and the order is by site, not by kind and site. Why: a
   lower-part patch needed the target of its `auipc`'s upper-part patch, found by a second binary search in
   another part of the list. A lookup is a read at a place no digest of the file in order covers, so a source
   that served other bytes to lookups than to the hashing pass could make the loader write code that
   differs from the signed bytes. With the target in the record, and the records in the order the image is
   relocated in, nothing is looked up. Cost: 4 bytes per patch (at most 32 KiB more at `MAX_PATCHES`;
   2 patches, 8 bytes, in the sample). The loader no longer checks that a lower part's `auipc` is patched
   (see above); the packer must.
5. **The write pass compares three digests** (header; consumed words; consumed patches), see "Size caps and
   memory". A mismatch is `Damaged` and the plugin is taken back. This replaces the earlier wording in this
   note that only the image and the tail were covered by the second read, and that single lookups into the
   lists were not.

## Slice 1

A host-testable reader in `paperpaw-native`: `Header5::read(bytes, host)` with the host's
`(target, abi_major, minor)` and a refusal for each check of steps 1 to 7 (wrong magic, older/newer
format, other chip, target 9, older/newer interface, needs newer minor, odd or truncated header,
nonzero flags or reserved, wrong file length, misaligned or overlapping parts, oversized counts),
and for the entry checks of step 9 as a function over the relocated bytes. Unit tests for each; no
firmware change. It does not depend on PR #186. Firmware wiring (slice 2) follows once this note
is merged.

## The packer (slice 3)

The packer's stack frame scan bounds each function's frame, not the depth of a call path; the band at the stack's end
and the device's high-water test cover that. A jump through a register inside a frame is how a `match` compiled to a
jump table looks; the scan reads such a table from the plugin's `.rodata` (the run of its relocated words that point
into the function) and follows every entry, and refuses one it cannot read. Plugins are built with
`-C jump-tables=no` all the same (set in the `rustflags` of the sample's `.cargo/config.toml`, the template of an
interface-7 plugin build), which leaves the plugin's own `match`es nothing to read; the flag does not reach the
precompiled `core`, whose tables the scan reads.

**Frame limit and floats (ADR 0034, decision #953):** a frame of 2,048 bytes or more is refused (the limit is the
band's height at the stack's end, `paperpaw_x4::stack_band::FRAME_LIMIT`); one of less cannot step over the band. The
panic handler's room is kept for the worst case of two such frames meeting in the band: the band lies
`RESERVE = DEPTH + FRAME_LIMIT` = 4,096 bytes above the stack's end, `DEPTH` being the larger of `HANDLER` (the panic
handler's depth) and `INTERRUPT` (an interrupt's depth on the main stack when it arrives with the pointer near the band),
each 2 KiB with a 256-byte margin until the device measures them together (checklist G7, and the interrupt case of the
overflow test G8); if either is more, the reserve grows, not the limit. Two assumptions the band rests on are not the
ABI's promise: `sp` moves in multiples of 16 (the packer refuses a plugin whose `sp` moves by anything else), and a
function's first store into its frame is its return address at the top (LLVM's habit; the worst case of two frames meeting
in the band needs a frame to cross into the band with no store there, which that allows). Band, reserve and
alignment take 6 to 8 KiB of the X4's 41,096-byte main stack. So `core`'s float formatting, whose
`float_to_decimal_common_exact` has a frame of 1,952 bytes, packs: `{}` and `{:.N}` of an `f32` or `f64` work on the
X4. The SDK's fixed-point formatter stays the recommended way (it saves roughly 10 KiB of flash), not a requirement.
If the deepest legitimate path does not fit the stack that is left (about 33 KiB), the limit falls back to 1,536
bytes and floats are refused again.

`paperpaw-sign pack <plugin.elf> <plugin.ppn> [--key <file>]` writes format 5 for a plugin linked with
`paperpaw-plugin7.x` and `--emit-relocs` that defines `PAPERPAW_METADATA`; a plugin that defines `PAPERPAW_API` is
interface 6 and gets format 4, and one that defines both or neither is refused. The file is made as above:

- the image is the linked `.rodata`, then `.text`; the words to relocate are the relocations that name an
  address in the image, by data or by code, the end included (as the loader takes them); the hot code and the
  patches that move it are format 4's, and **more than 8,192 patches are refused**;
- `entry` is `PAPERPAW_ENTRY` (exactly 20 bytes, 8-aligned) and `meta_at`, `meta_len` are `PAPERPAW_METADATA`
  (4-aligned, at most 2,048 bytes, a record this host reads); `abi_major` and `needs_minor` are the record's
  own. **Nothing is written for a draft** (there is no draft field), no fingerprint, no compiler;
- target 9 (**the X4 Pro and X4 Classic**: a plugin built for Xtensa) is packed, see "Target 9" below;
- the pack-time half of the freestanding rule: an undefined symbol, an exported symbol besides the two
  and the optional `PAPERPAW_HEAP` (`PAPERPAW_HOT` and `PAPERPAW_HOT_END` are the linker script's), a writable section or any section but
  `.rodata` and `.text` that takes memory (`.data`, `.bss`, TLS, `.init_array`, `.ctors`, `.fini*`), and a
  relocation kind outside the short list, are refused with the symbol or section named;
- `PAPERPAW_HEAP`, when present, is checked as in `abi/heap-word.md` §5 (in `.rodata`, 4 bytes, 4-aligned, 0, not
  a relocation word, outside the entry and the metadata) and sets `HEAP_WORD` and `heap_at`;
- before it returns, the packer reads the file as the loader does, at the base it is linked at and at another,
  with the hot code in the arena: the header, the digest, every word to relocate, and the entry and the
  metadata as they will lie once placed (`Relocator5::verify` and `check_entry`). A file it makes is one the
  X4's loader takes;
- `--key` signs the file as for format 4 (the signature follows the file; the digest does not cover it), and
  `plugin`, `sign-plugin` and `check-plugin` read both formats.

`samples/abi7-x4-min` is the smallest interface-7 plugin for the X4 (entry, record, a plugin that does nothing,
one hot function called from cold code). Its linked ELF and the file the packer makes of it are kept in
`crates/paperpaw-native/tests/data/` and read by the tests of the packer, of `Header5` and of `stream5`;
`samples/abi7-x4-min/build.sh --check` builds it again and compares. Not covered: a plugin that holds a
function pointer to hot code in its read-only data (the format-4 packer refuses that too), and the
Xtensa profile.

## Target 9: the X4 Pro and X4 Classic (draft 29, owner decision 142)

The same format 5, header, digests, signature and loader order; what differs for target 9:

- **Profile:** the metadata record's target is `esp32s3-xtensa-lx7-ilp32-windowed-le` (`abi/entry.md`,
  "The Xtensa profile"); a target-9 header with another metadata target is refused at step 9.
- **No hot code, no patches:** `hot_at`, `hot_len` and `patches` are 0. The loader refuses any other value as
  `Damaged` before it reads the image; the packer never writes them.
- **Relocations:** `relocs` lists the 32-bit words that hold a plugin address (`R_XTENSA_32`), increasing,
  each 4-aligned and inside the image, as format 4 did for the X4 Pro. The loader adds the distance moved to
  each and writes nothing else; the second (write) pass compares the header and relocation digests as for
  target 5 (decision 5), with an empty patch digest.
- **Placement:** the image is read into PSRAM, mapped for execution through `CODE_OFFSET`, and never written to
  flash (the X4's write-once plugin flash and its index are the X4's). `align` as for target 5.
- **Caps:** `image_len` at most format 4's `MAX_IMAGE_LEN`; the S3's PSRAM budget for all plugins is the
  shell's (Firmware), refused with "no room" before any byte is read, never by cutting.
- **Packer:** `tools/sign` pack5 accepts `EM_XTENSA` with the format-4 relocation table
  (`relocation.rs`, `Chip::Esp32S3`), refuses a call0 object (`.xtensa.info` `ABI=1` where present), and writes target 9.
- **Builds:** the release and store workflows build every code plugin for `x4-pro` (target 9; `x4-classic`
  takes the same file, as `store-plugin.yml` already maps it) with the esp toolchain, alongside `x4`.
  `tools/plugin-files7/build.sh x4-pro|x4-classic` packs every plugin's file (the same bytes for both
  devices, under their own names) against `native/metadata.s3.bin`, which `tools/plugin-files7/s3-metadata.py`
  writes from the plugin's own metadata (`--check` in CI). The S3 build sets `-C jump-tables=no` as the X4's.
- **Tests:** the loader's refusal sweep repeats for target 9 (nonzero hot or patch fields, a word outside the
  image, unsorted relocs, wrong metadata target, a target-5 file on the S3 and a target-9 file on the C3);
  `samples/abi7-x4-min` builds for both targets in CI. Device runs on an X4 Pro wait for the owner.
