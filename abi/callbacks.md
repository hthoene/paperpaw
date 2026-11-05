# M1 callback table increment

The existing PpPlugin size/state/drop prefix stays unchanged. Two optional
function pointers are appended, each since minor 1 of the unreleased interface:

- dispatch_event(state, token: u32, input: PpBytes) -> i32
- draw(state, token: u32) -> i32

Both use the target C calling convention. A callback returns zero for success
and a negative status for refusal/failure; positive values are invalid, not
lengths. A token belongs to this plugin instance, not a process-wide address.
All tokens for one state share the same mutable-state admission guard. Callers
must hold the owner/module lease and acquire admission before invoking either
function. These declarations do not activate runtime dispatch.

The host validates the accessible allocation span separately from the table's
size word. It must not form a reference to a full current PpPlugin over a shorter
allocation. Generated readers check the span, alignment, size and requested
field before copying its function pointer. A missing field or null callback is
unsupported. Malformed bounds are invalid. A reader neither invokes a function
nor validates executable addresses: valid pointer representations and executable
module lifetime remain caller obligations under the trusted native-plugin model.

Full table sizes are 40 bytes on x86_64 and 20 on the X4, with unchanged prefix
sizes of 24 and 12 bytes. PpEntry and metadata entry sizes do not change. Loader
minimum checks must continue to use the end of the drop field, not the new full
table size. Nested wrappers report only their own allocation and supported tail.

Event input is borrowed for the callback only. This increment defines no event
payload grammar, registration wire bytes or callback-generated output buffer.
Those codecs must be generated and reviewed before an actual event is delivered.
The eventual command path uses host commands.submit with bounded host-owned
staging; no output lease or repeated mutating size query is introduced here.
Drawing side effects retain the separate slot-14 non-atomic error contract.

Metadata requirement lists name callback dependencies explicitly with
`{"kind":"callback","name":"dispatch_event"}` or `draw`. The required minor
includes the callback field, enclosing plugin table and entry contract. Entry
presence alone must not stand in for requiring every callback added later.

Native linked C/Rust probes qualify argument/result exchange on x86_64. X4
probes assert layouts and compile types, not device calling-convention execution.
Short-table tests exercise the generated readers separately. None of this proves
live registered dispatch, aggregate resource use, or a complete Battery port.
