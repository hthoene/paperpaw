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
