# Owner lifetime increment

`Owner<C, M>` must be pinned before the loader calls `make`. `C` owns the host
context and its allocator/service dependencies; `M` is a loader-provided guard
which keeps executable code mapped. `HeldLibrary` stays private to the loader.
The successful make result is stored in C before `activate` arms its destructor.
An unsuccessful make remains the loader's cleanup responsibility.

A host registration or queued callback retains a `Lease` borrowing the pinned
owner. This is an allocation-free lifetime link: safe code cannot remove the
owner while such references remain. It is not a cloneable library handle or a
foreign wire value. Queues and registrations must be owned outside this owner;
self-referential owner-owned collections are not supported by this primitive.

Teardown first revokes admission, then removes registrations and cancels queued
work, releasing their leases. An in-flight callback also retains a lease. Finish
refuses until all leases are released, marks the owner dead, then calls the
plugin destructor exactly once. Context, host allocator/services and module stay
alive during destruction. Context drops before the module guard. Heap cleanup
must remain callable during plugin destruction; normal callback dispatch cannot
resume. The loader destructor wrapper must not unwind.

Every callback uses the same instance admission guard and the device-wide Depth.
Busy/depth/revoked errors occur before callback execution. Scoped FrameSlot
installation and cleanup remain the responsibility of the dispatch wrapper.

The eight-lease local bound is conservative experimental admission, not a new
wire limit or aggregate-memory qualification. The caller must account for owner,
registration, queue and module-guard storage, and use fallible owner allocation.

This increment is inactive. It does not install a host table, allocate a live
CoreCtx, supply module guards, parse registrations, define generated callback
records, or dispatch foreign code. Those integrations and their allocator pairing
and runtime memory tests are still required before the first callback proof.

## Shell wrapper and construction transaction

`paperpaw_core::native_host` exposes the host-only wrapper without exposing the
private lifecycle implementation. `HostOwner<C, M>` must be pinned before make;
its context view has a stable address. Firmware supplies the owned allocation,
context dependencies and opaque module guard. This is not a combined CoreCtx or
host table implementation.

Begin a `Construction<T>` before invoking make. Stage validated host records
there, with a maximum of eight records and externally enforced retained-byte
limits. Capacity failure poisons the transaction so commit rolls back all records. Records are not inspectable or dispatchable while staged. A full batch
returns the refused record to its caller. The loader must treat decoding or
staging refusal as a failed construction transaction, not commit a partial set.

After make returns, validate its output and store the successful plugin state in
C. Only then commit with the destructor thunk. Commit reserves the first lease and activates the owner without a fallible
step after ownership transfer, then returns a `RegistrationBatch<T>` with its own lease. No foreign code runs between
activation and lease reservation. Dropping the batch destroys its records before
releasing the lease. Additional queued work obtains its own callback lease.

If make or validation fails, abort the transaction before destroying any partial
foreign output. Staged record cleanup runs while the context and module remain
alive. The loader must handle partial-output ownership according to the entry
contract; the transaction does not call an unvalidated plugin destructor.
Revocation during make blocks later commit, and finish refuses while construction
is active. No early activation is permitted to obtain registration leases.

These are generic host records, not a substitute for generated callback tables,
wire validation or registration codecs. Publication into global registries,
fallible owner allocation, module guard implementation, scoped FrameSlot setup,
foreign dispatch and aggregate resource accounting still require integration.

### Successful make followed by commit rejection

The loader owns a validated successful make result until commit returns Ok.
Every commit Err leaves owner destruction unarmed and drops staged records before
returning. The loader must then invoke the validated output destructor exactly
once while the context and module are still alive. This applies to both poisoned
capacity and revocation errors, even though make itself succeeded. Owner finish
or drop does not clean this rejected output. Keep the output in its loader guard
until Ok transfers it; disarm that guard only on Ok.

A failed make or invalid output is a different case: abort staging, then follow
the entry contract for partial output. Never call an unvalidated destructor.

### Scoped access during make

The pinned context may contain `native_host::registration_scope::RegistrationScope<T>`
for owned, validated host records. The loader installs its external construction
transaction only around the synchronous make call. Host submission borrows the
transaction for the submission itself; no registry borrow is retained across a
foreign call. Nested installation is refused. The scope clears on return and
unwind, before the transaction borrow ends. Submission outside that scope returns
the refused record unchanged.

`RegistrationScope::reject` records a refusal without borrowing the staging
transaction. Later submissions are refused. On every scope exit, including
unwind, reset clears access before the target propagates its refusal through
`Construction::reject`. Commit then returns Invalid, preserving any earlier
capacity failure instead. Ignoring the original host status cannot publish the
partial registration set. Only aborting and starting a new transaction clears
the latch; validated foreign output remains loader-owned on commit rejection.

This bridge does not decode bytes, validate callback pointers, or publish a
registration. The future slot-16 adapter must validate the complete generated
batch and account for retained storage before staging, and call `reject` on
every decoding, validation or resource refusal. That adapter and actual foreign
invocation still require tests and review before raw registration is wired.
