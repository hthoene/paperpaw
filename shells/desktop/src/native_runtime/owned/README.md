# Inactive desktop owner integration

Assignment #718, ticket #228. This module is separate from both the production
`CodeLoader` and PR #142's process-owned inactive `load` API.

Dependencies: PR #142 at `277b4e5e3` and Kernel's lifecycle branch at `49831634`.
The latter includes the reviewed `4536584b` ownership contract plus opaque Debug
implementations. Neither dependency's frozen source is modified here.

The checked library and its open file become a private module guard inside a
fallibly allocated, pinned `HostOwner`. Its concrete context owns the generated
host table and the heap/clock services. Four typed bridges decode that exact
context and forward through a temporary `ShellCtx`. A temporary pointer never
escapes the forwarding call. There is no CoreCtx/ShellCtx prefix cast, leaked
service object or lifetime extension.

Begin the external construction transaction before make. Validated output is
stored in the pinned context, with a loader-owned pending guard that also owns staging. Abandoning that guard
drains staging before destroying foreign output. A successful
commit transfers destruction to the owner. Every rejected commit drains staged
records first, then the guard invokes the validated destructor once while the
table, services and module are alive. The guard checks that the transaction
belongs to the same context. Failed make must return no live output; null or
invalid successful output never authorizes calling an unvalidated destructor.
No malformed foreign output cleanup is inferred from readable-looking bytes.

Published batches and queued leases borrow the owner from outside. Revoke
admission, remove batches and cancel queues, then finish. Finish remains Busy
while either lease remains. The foreign destructor can still use the host heap
and clock. Context/services drop before the module is unloaded.

The Linux real-library tests compile no_std cdylibs with the generated ABI and
`-nostartfiles`, then use the same bytes-first checks and real dlopen path.
Independent host counters observe foreign make/destruction, allocation/free,
record cleanup and context release. `/proc/self/maps` checks confirm mapping
presence during service/context cleanup and absence after owner drop. No second
library handle is opened for observation. Tests cover successful construction,
revocation and queued leases, capacity-poisoned and revoked commit, factory
failure, null successful output and owner-drop teardown and abandoned pending construction.

The original transaction tests retain host-only records. A separate registered
make path uses the pinned context's RegistrationScope<Registration>, installed
only around synchronous make. Its explicit typed host bridge forwards raw batch
pointer/length to the reviewed Kernel decoder. Complete decoding/support checks
precede staging; active refusal latches transaction rejection even if ignored.
The scope clears before factory output validation or pending-output commit.

The foreign child tests build registration/event bytes using the generated
encoders, compile a real no_std library invoking registrations_submit during
make, and dispatch its event/draw callbacks only through admitted leased batches
or queued leases. Independent one-byte heap markers distinguish callback effects
from instance allocation/free. Cases cover valid commit, ignored malformed,
unsupported and capacity refusals, invalid event rejection, out-of-scope submit,
and revoke with Busy finish until all leases drain. These implementation cases
still require execution and independent review before acceptance.

Explicit dependency: registration wire PR #152 at cdc9e2da and prefix-loader
PR #151. Production CodeLoader, application registry publication, FrameSlot/Env
service integration, returned commands/screens and Battery migration are not
activated or qualified by this isolated tick/draw heap-marker proof. No real
hardware or aggregate 256 KiB device-memory result is implied.

Targeted verification:

```sh
cargo test -p paperpaw-emulator --lib --no-default-features native_runtime -j4
cargo clippy -p paperpaw-emulator --all-targets --no-default-features -j4 -- -D warnings
```

Run full `tools/ci-local.sh full -j4` before publishing. These are host proofs;
no hardware or X4 memory qualification follows from them.
