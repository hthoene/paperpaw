# Draft M1 registration and tick records

`abi/abi.def` owns the record tags, field order and field types. The closed `record_codec` descriptor owns the
registration count cap and its introduction minor. The generator
validates the supported initial shapes and writes identical standalone modules
to `abi/generated/records_wire.rs` and
`crates/paperpaw-core/src/abi7/records_wire.rs`. The existing raw ABI module is
unchanged. Lock format 4 separately covers the complete emitted codec body.

A batch starts with an unsigned canonical u32 LEB128 record count. Each record
is a canonical u32 LEB128 tag, a canonical u32 LEB128 body byte length, then that
many body bytes. The body starts with a canonical u32 LEB128 bitmap byte length,
then that many presence bitmap bytes, then positional scalar fields. Scalars
are fixed-width little-endian integers. Encoders write a zero-length bitmap.
Overlong, overflowing or truncated varints are malformed.

| Tag | Record | Required fields, in order |
| --- | --- | --- |
| 1 | `registration.event` | `event_tag: u32`, `token: u32` |
| 2 | `registration.draw` | `token: u32` |
| 3 | `event.tick` | `now_ms: u64` |

All declarations and fields start at minor 1. There are no optional fields yet.
Unknown bitmap bits, extra bitmap bytes and trailing fields inside a known
record are accepted for forward compatibility. Known required fields must still
be present after the bitmap. Bytes outside the declared batch are malformed.
Unknown registration tags return `ERR_UNSUPPORTED`; tick decoding requires
exactly one record with the tick tag. It accepts the same bounded extensions.

`Registration` is a copyable, non-exhaustive enum with `Event { event_tag,
token }` and `Draw { token }`. `registrations_decode(&[u8])` returns a completely
validated `RegistrationBatch` backed by eight optional records. Its `iter()`
yields registrations by value, and it exposes `len()` and `is_empty()`.
`registrations_encode(&[Registration], &mut [u8])` returns bytes written.
`tick_encode(u64, &mut [u8])` returns bytes written, and
`tick_decode(&[u8])` returns milliseconds as `u64`. All results use `i32` errors.
`EVENT_TICK_TAG` is emitted from the schema for host event dispatch.

The byte limit is `REGISTRATION_MAX_BYTES = 4096`; registrations also have
`REGISTRATION_MAX_RECORDS = 8`. Exceeding either limit returns `ERR_NO_SPACE`.
Malformed input returns `ERR_INVALID`. An insufficient output buffer returns
`ERR_NO_SPACE` before any byte is written. Both encoders check all bounds before
writing and leave unused output bytes unchanged. The codecs allocate nothing
and retain no borrowed input. Callback-token ownership and supported event-tag
admission are host responsibilities, separate from structural decoding.

Golden vectors, malformed framing, byte/count boundaries, atomic output errors,
and forward extensions are checked with:

```sh
python3 -m unittest discover -s tools/tests -p test_abi_records.py -v
```

The test compiles the generated module as a standalone `no_std` library and
runs its Rust tests. This codec does not qualify the aggregate M1 memory budget.
