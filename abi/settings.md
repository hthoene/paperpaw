# M1 settings copy-out value

Slot 5 keeps its key and caller-buffer signature. The generated
`settings_value` descriptor in `abi.def` owns its byte contract and limits.
This is a typed value, not configuration-file text. Current settings have three
value variants; absence is a separate wire case. List settings remain text.

Layout 1 is little-endian: a u32 layout, a u32 kind and a u32 payload length,
followed by exactly that many payload bytes. Kind 0 is absent with no payload;
kind 1 is a one-byte Boolean (0 or 1); kind 2 is an eight-byte signed integer;
kind 3 is UTF-8 text without a terminator. Empty text, false and integer zero are
all distinct from absence. Unknown layouts or kinds are unsupported, not absent.

The complete result may use at most 4,096 bytes, including its 12-byte header.
The host returns the complete required length and copies only the prefix that
fits. A zero-capacity query writes nothing. A partial result may contain only
part of the header or UTF-8 code point: the facade must decode only a complete
response. Oversized values fail before any output is written. Input pairs and
output capacity are validated before a scoped settings or translation lookup.
Malformed input and errors leave output storage untouched.

The borrowed codec itself allocates nothing. The adapter deliberately uses
`Settings::get` to preserve defaults, scope, previews and placement overrides;
that existing lookup can allocate. There is no allocation-free lookup claim.
Translation copying similarly preserves `Translator::tr`, including returning
the key when missing, and copies before performing another lookup. No borrowed
catalog string is returned across the ABI.

These backends compile in core but are not installed in a combined host table.
Stable owner lifetime, module leases, raw callback dispatch and representative
plugin qualification remain separate gates.
