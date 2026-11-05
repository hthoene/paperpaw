# X4 text memory probe (#228)

This test harness compares one direct `FontHandle::draw` with one call to the
reviewed slot-14 safe backend. It does not load a plugin or enable the production
flat interface. The backend source is `crates/paperpaw-core/src/abi7/graphics.rs`.
The input/font construction follows #228 attachments 699/700. Device procedure
and fixture identities are attachments 704–706. A reviewed image and a separate
Manager execution assignment are required before a device run.

## Build without running or flashing

From `shells/xteink`, with the pinned Rust toolchain:

```sh
unset RUSTFLAGS
CARGO_TARGET_DIR=/path/to/own-target cargo build --release -j4 \
  --no-default-features --features x4,textprobe
sha256sum /path/to/own-target/riscv32imc-unknown-none-elf/release/paperpaw-xteink
```

`textprobe` requires `remote`. It is absent from default/release feature sets.
No `cargo run`, serial port or flash command is needed to produce the candidate.
Record commit, toolchain, feature list, ELF SHA-256 and size before review. An
ELF hash is not a flashed binary hash: any later image conversion must be pinned
and hashed separately by the authorized device procedure.

## Command and identities

The bounded command grammar is:

```text
textprobe <case> <direct|adapter|control> <cold|warm> <clean|fragmented> <raster|describing|blind|rtl-ink> [repeat10]
```

`bitmap-A` is an additional control, not a replacement for the pinned lowercase
ASCII payload. It uses uppercase A repeated 4096, SHA-256
`6896d9ea3f73a4434f5832bc65714e7d066f177373f36f34dc8a6f735daa41b1`.
The real fixture contains ?, A and é, but no lowercase a. The historical
`bitmap-ascii` therefore does not prove stored-glyph/cache coverage; only the
new covered-glyph control can make that claim. Host tests check coverage and
visible pixels. Plugin/Device Tester have been notified of this evidence gap.

Each invocation creates a new Fonts registry, 4096-byte input and 40x32 Gray8
framebuffer before measurement. Blind uses the matching blind frame. Warm does
exactly two unmeasured direct draws on this fresh registry. Cache size is 4096;
ligatures are enabled; size=12, x=0.125, baseline=16, color=WHITE. RTL ink uses
RIGHT_TO_LEFT. `control` runs the same measurement wrapper without drawing.
It is an observer baseline, not an output-parity reference. `repeat10` is accepted
only with warm: ten separate measured draws retain the same registry/frame and
fragmentation buffers after the two warmups; no result Vec is retained. SAMPLE
and RESULT carry repeat=0..9; serialization occurs after each closed scope.

| Harness case | External manifest case | Font setup | Input SHA-256 |
|---|---|---|---|
| ascii | control-bitmap | synthetic all-script | c93eee2d0db02f10acc7460d9576e122dcf8cd53c4bf8dfcae1b3e74ebcfff5a |
| bitmap-ascii | control-stored | synthetic Hebrew primary, real stored fixture extension | c93eee2d0db02f10acc7460d9576e122dcf8cd53c4bf8dfcae1b3e74ebcfff5a |
| hebrew | hebrew | synthetic all-script | 6a53c5fc20760eb28b198288ed6668066e89dcf560a06b6ffd99c45b6e860765 |
| rlm-ascii | single-bidi | synthetic all-script | 2133bde9be9f92a5ce44367b4a6fd8415e69c93830e9e863fb62cf892c0f495a |
| rlm-alternating | fallback-parts | punctuation primary plus stored ASCII/script extension | b608850c0392486b189305f227765f606129f5e83c3a305bd263e6e15dc57028 |

Every input is exactly 4096 UTF-8 bytes with no newline or terminator. The real
bitmap fixture is `crates/paperpaw-text/testdata/pack.ppfp.v1`, 211 bytes,
SHA-256 `98ac905376d3246a88a605724e3a72f2e8bef359a240c7a0131890eaa938c7f5`.
Stored fonts use MemoryStorage. Installed reading fonts, SD/FAT buffers, normal
reader font geometry and arbitrary font/cache sizes are not covered.

`fragmented` is an **artificial** allocation profile: allocate sixteen 1024-byte
Vec buffers, then release even-indexed buffers and retain eight during drawing.
It is distinct from font fallback splitting and from the representative reader
history required by attachment 705. `clean` adds none of these buffers; the
normal reader remains alive in both cases. Pin reader state externally.

## Measurement records

The response is `PP:TEXTPROBE_BEGIN`, `BEFORE`, `AFTER`, `RESULT`, `LIMITS`, `END`, then the standard `PP:END` terminator.
Each line uses space-separated `key=value` fields. An error uses
`PP:TEXTPROBE_ERROR reason=<token>` followed by END; missing result fields mean
incomplete measurement, never zero or a pass. BEGIN includes case/path/cache/
heap/mode, fixture hash and font profile. AFTER/RESULT are emitted only after
measurement closes. Formatting, hashing, assertions and largest-block probes
are outside the scope. No host-side polling is needed during the synchronous draw.

BEFORE/AFTER report used/free, largest block and since-boot stack left/size.
Used/free are sampled in one critical section. Stack is sampled before the
intrusive largest-block probe. The existing probe has 64-byte resolution and
uses Vec alignment; this is a lower-bound availability estimate at that alignment.
It cannot certify availability for arbitrary requested alignment.

RESULT reports exact f32 advance bits, framebuffer SHA-256, baseline/end/peak
actual heap used, successful allocation count/request traffic, largest successful
request size, deallocation/freed-request counts, allocation failures, counter
overflow and elapsed microseconds. The scope observes esp-alloc allocation hooks,
including allocator overhead in actual used bytes and allocations on realloc
paths. Request metrics use layouts, not allocator charged sizes. Diagnostic probe
allocations are excluded. Unrelated interrupt activity is included; quiesce radios
and record remaining background activity externally. Saturation invalidates exact
comparison. Derive actual total from atomic used+free, minimum free from total
minus scoped peak and retained delta from end minus baseline.

The BEGIN/BEFORE records precede the risky draw so a reset does not erase its
identity. Renderer allocation remains infallible. Before measurement, conservative
harness admission requires both free and one contiguous block >=96 KiB for the fixed assets.
The earlier 80/52 KiB proposal was rejected: a Part-sized largest block can
leave no contiguous space for later simultaneous order buffers. The new reserve
still needs source review and target evidence before device admission. This is a test guard, not an arbitrary-font bound
or production policy. An insufficient-headroom refusal is incomplete/failing
qualification. The input is never reduced or split to obtain a passing result.

## Open qualification gates

The initial harness reports stack watermark **since boot**. It does not establish
a <=512-byte added boundary stack delta. Separate scoped-stack instrumentation
is under development and requires review before that capability can be claimed.
The esp-alloc hook does not expose request alignment; it is unsupported. The
font describing flag is exercised, but semantic sink hashes are unsupported.
LIMITS carries explicit unsupported markers. Do not replace those with zero.

Compare matched fresh direct/adapter invocations, reverse order across fresh
replicates, and retain worst peak/minimum-stack results. Existing renderer scratch
is common to both paths; adapter-minus-direct is the added backend cost. Neither
is the complete flat runtime overhead. The project gates remain <=222822 bytes
peak for the 256 KiB whole-runtime heap, >=10240 unused stack, <=4096 bytes
aggregate added boundary heap, and no added per-draw boundary allocation.
A baseline missing normal reader/SD/font state cannot close aggregate runtime gates.

Raw slot-14 handle/UTF-8/context dispatch, module leases, other host entries,
installed fonts, representative heap history and real X4 execution remain outside
this isolated backend comparison. Old prototype v5 is not a candidate or baseline.

## Fixed-asset headroom review

The 96 KiB contiguous reserve is pending target attestation. Source review bounds
allocation **traffic**, including replacement blocks during Vec growth, rather
than using only peak live demand. For the alternating case, with actual private
`Part` size S and text-cache `Entry` size E, the conservative formula is:

```text
q(4094) + q(4094*S) + q(4094) + q(8188)
+ q(460) + q(16) + q(32) + q(20) + q(4*E)
q(n) = round_up_to_4(max(n, 8))
```

The conditional maximum is 82668 bytes when S<=16, E<=64, their alignments<=4
and bidi Level remains one byte. The candidate must attest these private target
layouts using the actual compiler output, not an inferred/copy definition. The
pinned esp-alloc linked-list first-fit allocator consumes aligned hole prefixes;
a bound on all charged traffic preserves enough of the initial contiguous block
even when other requests fit smaller holes. This argument is restricted to the
fixed synthetic/real-bitmap assets and no allocating storage reads, marks or
outline rasterization. It does not certify arbitrary fonts or reader histories.

A reproducible validation route uses the pinned compiler's type-size diagnostic,
in a separate build target, without changing source or toolchain:

```sh
RUSTC_BOOTSTRAP=1 cargo rustc --release -j4 -p paperpaw-sdk \
  --no-default-features --features kernel --target riscv32imc-unknown-none-elf \
  -- -Zprint-type-sizes
RUSTC_BOOTSTRAP=1 cargo rustc --release -j4 -p paperpaw-text \
  --no-default-features --target riscv32imc-unknown-none-elf \
  -- -Zprint-type-sizes
```

Record the actual Part/Entry/Level lines, target feature set, dependency lock and
source commit. Diagnostic builds are not the candidate firmware image. Do not
claim numeric admission is verified until this evidence and the allocator
configuration are independently reviewed. Admission must remain atomic with
respect to allocation by interrupts/tasks before every warmup and measured draw.
