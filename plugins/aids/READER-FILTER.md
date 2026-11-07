# reader.filter schema 1 preparation

Opt-in `reader-filter` feature; default registrations still use the existing
interface. No stage broker, native activation, or settings validation UI is
claimed. This is the reader-owned payload implementation housed with its first
consumer while the reader port is pending.

Local tags: 1 rules (signature u64, bold 0/1, image policy 0 keep/1 caption/2 drop,
localized caption word); 2 ordered whole-word replacement (from, to, flag 1).
One rules record first, then at most 32 pairs: 33 records, below STG's 64-record
ceiling. Each string is at most 96 UTF-8 bytes; whole payload at most 3840 bytes.
Both limits apply: 32 full-length pairs do not fit (about 17 fit, depending on
the header). Settings validation must measure the exact encoded payload and
refuse the whole configuration visibly when either limit is exceeded.
The generated framing is shared with the approved #224 codec template. Regenerate
with `python3 tools/reader-filter-gen.py`; `--check` detects drift. ABI global
records and lock are unchanged.

Caption word is localized once; the reader combines it with each image's own
trimmed alternative text, preserving brackets, centre/italic styling and source
positions. Image-only cbz/cbr/xtc/xtch books bypass image policy. Apply returns a
drop verdict; consumers must honor it. Bold precedes ordered replacements, as
in the existing registrations. A complete block, including source mapping, is
the application unit; never apply independently to arbitrary stream chunks.

No malformed reply changes a block. Signature hashes canonical rule bytes with
its signature zeroed. Reader cache integration must additionally include the
registration signature and rerun after book open and relevant setting/plugin
changes. User style grants are not required for these content transforms.

Oversized legacy replacement settings cannot be silently truncated. Adoption
still requires visible whole-configuration refusal under the approved #964
policy. The default interface retains legacy behavior until that UI and broker
exist. Allocation failure/embedded stack and runtime timing remain device gates;
this change supplies host behavior proofs, not X4 qualification.

## Atomic application follow-up (#1028, todo 37)

The new application uses `Block::try_transform` from public
`paperpaw-document`. It never calls the mutating legacy filters. The current
64 KiB additional-working-memory budget is split between the document transaction
and caller scratch. It excludes the original block and input payload; this is
not a measured total X4 heap bound. Reservations are fallible and checked before
growth. Ordered replacement expansion returns `Limit` and leaves the original
block unchanged. Matching positions are captured before each rule, preserving
adjacent punctuation behavior.

Job #3415 passed 23 unit tests, two refusing-allocator tests, formatting and
all-target clippy with `reader-filter`. The allocator sweep fails successive
allocations until the complete caption application succeeds; every refusal
keeps the original block. Caption text/style/kind/alignment/image changes commit
atomically. Its source map deliberately changes: the caption spans the original
picture's source range instead of retaining stale alternative-text checkpoints.
The en/de caption regression asserts that mapping separately.

This is inactive preparation pending review, generator migration and cache-key
validation below. Existing registration remains unchanged in this intermediate
branch; the final native port must remove it, not retain a compatibility path.
No emulator activation, device heap/stack or timing qualification is claimed.

## Kernel review gates

Before merge, migrate to Core PR #276's single stage-schema format and generator
after it lands in integration. Use `key: reader.filter`, major 1, explicit input
and reply lists and limits, and `since` starting at 0. Keep generated output
plugin-owned; remove the separate `abi_stage_codec.py` and `reader-filter-gen.py`
path, preserving its regression coverage in the shared generator.

Before activation, stop trusting the sender-provided signature. Prefer removing
the field and deriving the cache key at the receiver; if retained, verify FNV-1a
64 over the canonical encoding with that field zeroed before any mutation. The
current application does not verify it and must not supply a trusted cache key.
