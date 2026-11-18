# Foreign registration fixture preparation

`registration.rs.in` is a no_std real-library source template for QA H1/H2/H4.
The foreign test harness supplies it to rustc and real dlopen; production
CodeLoader does not consume this template. Execution/review evidence is separate.

The test harness supplies ABI path, valid metadata/minor, opaque token,
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
heap marker. Evidence must count these separately from larger instance
allocation/free, make/destructor clock calls, registration cleanup and mapping
release. H4 must show unchanged callback counters after revoke and Busy finish
until both published records and queued leases drain.

The pinned desktop context must own the reviewed RegistrationScope, and its
explicit typed registration bridge must call Kernel's checked raw-pair decoder.
Scope installation wraps only synchronous make and must clear before pending
commit or cleanup. No mutable registry/context borrow may cross foreign code.
The integration explicitly depends on reviewed, published registration wire
PR #152 at cdc9e2da; no hand-written encoding or decoder substitute is used. Production
CodeLoader, real hardware, Battery migration and actual foreign dispatch remain
gated. Compilation alone does not establish any H1/H2/H4 outcome.
