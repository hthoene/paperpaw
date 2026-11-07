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
The checked-in codec is provisional output from the earlier #224-based generator.
That separate generator is not included here, so there is no supported regeneration
command on this branch yet. Kernel follow-up 38 must provide the shared external-schema
entry point before migration. ABI global records and lock are unchanged.

Caption word is localized once; the reader combines it with each image's own
trimmed alternative text, preserving brackets and centre/italic styling. Caption
source positions use a fresh identity map from the picture start; the text end maps to the picture end. Image-only cbz/cbr/xtc/xtch books bypass image policy. Apply returns a
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
`paperpaw_sdk::document`. It never calls the mutating legacy filters. The current
64 KiB additional-working-memory budget is split between the document transaction
and caller scratch. It excludes the original block and input payload; this is
not a measured total X4 heap bound. Reservations are fallible and checked before
growth. Ordered replacement expansion returns `Limit` and leaves the original
block unchanged. Matching positions are captured before each rule, preserving
adjacent punctuation behavior.

The allocator sweeps fail successive allocations in both caption and Paragraph
bold application; every refusal keeps the original block. Caption text, runs,
kind, alignment and image changes commit atomically. `TextEdit::try_set_text`
resets stale alt-text checkpoints: caption index i maps to picture start + i,
and the caption end maps to the picture end. Empty-alt and nonidentity-map
regressions cover this deliberate source-position correction. Text, styling and
image-policy behavior still match the existing en/de fixtures.

This is approved only as inactive preparation under the accepted gates. Generator migration and final native registration remain open. Existing registration remains unchanged in this intermediate
branch; the final native port must remove it, not retain a compatibility path.
No emulator activation, device heap/stack or timing qualification is claimed.

## Kernel review gates

Before completing the port, migrate to Core PR #276's shared stage-schema format
and generator once follow-up 38 supports plugin-owned schemas. Use `key: reader.filter`, major 1, explicit input
and reply lists and limits, and `since` starting at 0. Keep generated output
plugin-owned. No duplicate generator is present on this branch; preserve the
previous generator regression coverage when adopting the shared entry point.

The receiver now verifies FNV-1a 64 over the canonical generated encoding with
signature zeroed before mutation. Stale signatures return Invalid. Both
validation buffers are fallibly allocated and dropped before the transaction;
allocator sweeps include these allocations. Shared-generator migration must
preserve this verification or remove the transmitted signature and derive the
cache key entirely at the receiver.
