# Environment snapshot wire format

`abi/abi.def` owns the `snapshot_value` descriptor. `tools/abi-gen.py` emits
an independent, allocation-free Rust codec in `abi/generated/snapshot_wire.rs`
and `crates/paperpaw-core/src/abi7/snapshot_wire.rs`. No Rust struct layout
crosses the ABI. The original raw ABI module stays unchanged.

Host slot 4, `env.snapshot`, is a status operation with a 128-byte output cap.
The encoder writes a complete 52-byte version 1 snapshot. The caller must
provide that much storage; a short buffer returns `ERR_NO_SPACE` and receives
no writes. This slot has no length query or partial-copy behavior. The encoder
accepts uninitialized output storage and initializes exactly the returned
prefix, leaving spare capacity untouched. It validates the complete value
before writing. Percentages above 100 return `ERR_INVALID`.

All multibyte fields use little endian. Offsets below follow the descriptor's
ordered header and field lists; the generator computes them from field widths.
There is no alignment padding.

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 4 | Version, currently 1 |
| 4 | 4 | Total bytes, currently 52 when encoded |
| 8 | 4 | Presence bitmap |
| 12 | 4 | Reserved, zero |
| 16 | 8 | Monotonic milliseconds, unsigned |
| 24 | 8 | Unix seconds, signed |
| 32 | 2 | Display width in pixels |
| 34 | 2 | Display height in pixels |
| 36 | 2 | Display pixels per inch |
| 38 | 2 | Pixel kind |
| 40 | 2 | Shell-defined pixel code |
| 42 | 1 | Bits per pixel |
| 43 | 1 | Battery percentage |
| 44 | 2 | Battery millivolts |
| 46 | 1 | Charging Boolean |
| 47 | 1 | Battery saver Boolean |
| 48 | 1 | Touch input Boolean |
| 49 | 2 | UTC offset in minutes, signed |
| 51 | 1 | Reserved, zero |

Presence bits are battery `1`, voltage `2`, Unix time `4` and UTC offset `8`.
Voltage requires battery presence. An absent Unix time requires zero seconds.
The UTC offset is the minutes the user's time zone is ahead of UTC now (east
of it, summer time included): `core.time-zone`, else the system's zone, as the
SDK's local time reads it; -720 through 840 (UTC-12:00 to UTC+14:00), absent
while the device knows no time or the offset is outside that range (never
clamped). An absent offset requires zero; a value outside the range is
`ERR_INVALID` in both directions. Stats and reading goals take a day's
boundary from it (`unix + 60 * offset`). An absent battery
requires zero percentage and false charging and saver flags. An absent voltage
requires zero millivolts. These canonical zeros preserve the distinction
between absence and a present zero value. Battery percentages are 0 through
100 inclusive. Boolean bytes must be exactly 0 or 1.

| Pixel kind | Meaning | Required bits | Required code |
| --- | --- | --- | --- |
| 0 | Mono1 | 1 | 0 |
| 1 | Gray2 | 2 | 0 |
| 2 | Gray4 | 4 | 0 |
| 3 | Gray8 | 8 | 0 |
| 4 | Rgb565 | 16 | 0 |
| 5 | Rgb888 | 24 | 0 |
| 6 | Other | Any u8, including zero | Any u16, including zero |

The local `PixelFormat` facade preserves the SDK's depth semantics:
`bits_per_pixel()` returns the known depth or the exact `Other.bits` value;
`gray_levels()` returns 2, 4, 16 or 256 for the grayscale formats and `None`
for colour and `Other` formats.

The decoder requires an input length of 52 through 128 bytes and a declared
total equal to that length. It decodes the known prefix and accepts arbitrary
trailing extension bytes under version 1. Unknown version, presence bits or
pixel kind return `ERR_UNSUPPORTED`. Input above 128 bytes returns
`ERR_NO_SPACE`. Truncation, noncanonical absent payloads, invalid known pixel
depths or codes, invalid Booleans, invalid percentages, nonzero reserved bytes
and inconsistent declared lengths return `ERR_INVALID`.

The descriptor is closed. Adding it must respect the ABI minor release floor;
an existing descriptor cannot be removed or changed. Draft 29 is the last
change inside the 52-byte prefix (the UTC offset in the tail's reserved
bytes, presence bit 8): after 1.0 the snapshot grows only by trailing
extension bytes under version 1, read by the length the host declares, and
never changes a byte of the prefix. Lock format 5 records
its separate emitted source as `emitted_snapshot`. Lock formats 1 through 4
remain readable without this field. Generation does not insert snapshot
facades or codecs into the existing raw Rust module.

Focused validation:

```sh
python3 -m unittest discover -s tools/tests -p test_abi_snapshot.py
```

This includes standalone `no_std` compilation with warnings denied, Rust wire
vectors, output initialization and capacity checks, every pixel format,
malformed prefix checks, accepted extensions and descriptor release floors.
The codec allocates no heap memory and uses one 52-byte encoding buffer.

## Host integration

A pinned per-plugin context owns `native_host::environment::EnvironmentScope`.
Call `scope(env, callback)` around synchronous dispatch, then forward slot 4
through unsafe `copy(out.ptr, out.cap)`. The caller must provide live exclusive
output storage disjoint from host state; uninitialized output is supported.
The host method returns `OK` on success; the generated `total_bytes` header
field carries the encoded length. The standalone encoder returns a byte count. Short buffers
fail without writes; no active environment returns `ERR_STATE` for valid output.
Scope cleanup runs on normal return and unwind, and nested installation is Busy.
Only owned snapshot values leave the borrowed frame.

Capture uses the current Env's device, SystemStatus and monotonic Instant. Unix
seconds use the same wall-clock mapping and floor division as SDK UnixTime.
Pixel format is preserved, including vendor Other code/bits; the facade retains
its existing gray-level fallback policy rather than assigning it in the codec.
This supplies current-state values only. Full Battery also needs storage, usage,
plugin metadata/work, richer i18n/UI and the real foreign callback proof.
