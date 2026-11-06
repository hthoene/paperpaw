# Foreign registration fixture preparation

`registration.rs.in` is a no_std real-library source template for QA H1/H2/H4.
It is not linked into the emulator and has no runtime qualification yet.

The future test harness must supply ABI path, valid metadata/minor, opaque token,
valid/invalid registration batches and expected event bytes. Registration/event
bytes must be emitted by the reviewed generated codec or its checked negative
fixtures. Empty substitution arrays used to check Rust compilation are not wire
fixtures and must never be called a successful registration test.

The plugin calls the named generated `PpHost.registrations_submit` field during
make, without a numeric slot offset. One mode returns on its first refusal. H2
submits valid then invalid input, deliberately ignores the second status, and
returns a validated live instance. The host must reject commit, drain staging,
then destroy/free that instance once. Decoding must validate the complete batch
before staging; every refusal must latch rejection even if the plugin ignores it.

Both foreign callbacks check the opaque token; event also compares its borrowed
input with generated expected bytes. A callback allocates/frees a one-byte host
heap marker. Future evidence must count these separately from larger instance
allocation/free, make/destructor clock calls, registration cleanup and mapping
release. H4 must show unchanged callback counters after revoke and Busy finish
until both published records and queued leases drain.

The pinned desktop context must own the reviewed RegistrationScope, and its
explicit typed registration bridge must call Kernel's checked raw-pair decoder.
Scope installation wraps only synchronous make and must clear before pending
commit or cleanup. No mutable registry/context borrow may cross foreign code.
No scope/codec source is consumed here before its checked publication. Production
CodeLoader, real hardware, Battery migration and actual foreign dispatch remain
gated. Compilation alone does not establish any H1/H2/H4 outcome.
