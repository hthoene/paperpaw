# reader.filter schema 1 preparation

Opt-in `reader-filter` feature; default registrations still use the existing
interface. No stage broker, native activation, or settings validation UI is
claimed. This is the reader-owned payload implementation housed with its first
consumer while the reader port is pending.

Local tags: 1 rules (signature u64, bold 0/1, image policy 0 keep/1 caption/2 drop,
localized caption word); 2 ordered whole-word replacement (from, to, flag 1).
One rules record first, then at most 32 pairs: 33 records, below STG's 64-record
ceiling. Each string is at most 96 UTF-8 bytes; whole payload at most 3840 bytes.
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

## Blocking code review #1028

The initial application implementation still calls allocation-heavy legacy
filters. The wire cap does not bound output expansion across ordered replacements.
It must not be wired into the native reader until a checked allocation, bounded
working transaction replaces that path and proves refusal leaves the original
block unchanged. This is a code correctness gate, not an X4 measurement deferral.
Encoder reservations are now fallible; that alone does not resolve application.
