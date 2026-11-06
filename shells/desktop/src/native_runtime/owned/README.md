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

Registration values in this increment are host-only test records. They are
staged before make and cannot dispatch until commit; this does not prove foreign
`registrations.submit` decoding or staging during make. Kernel owns the generated
callback record schema and the scoped active-transaction context seam. There is
no invented wire slot, foreign callback dispatcher, global registry publication,
or production activation here. The next runtime gate is one real plugin calling
the generated registration bridge during make, followed by admitted foreign
callback dispatch and refusal after revoke, using these same ownership guards.

Targeted verification:

```sh
cargo test -p paperpaw-emulator --lib --no-default-features native_runtime -j4
cargo clippy -p paperpaw-emulator --all-targets --no-default-features -j4 -- -D warnings
```

Run full `tools/ci-local.sh full -j4` before publishing. These are host proofs;
no hardware or X4 memory qualification follows from them.
