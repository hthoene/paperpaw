# The host heap word: a global allocator for interface-7 plugins

Status: **contract for review** (#228; found by Plugin with Battery #923; option (A) agreed by Senior
Reviewer with conditions 1–9 on #228; Manager: Senior's OK suffices, recorded as an amendment of
ADR 0034 below). Kernel owns this text, `abi.def` and the generated outputs; Firmware owns the X4
loader and `paperpaw-sign pack`; Core owns the host allocator, the frame owner and the desktop loader.

## 1. The gap

`abi/entry.md` hands the host table and context to a plugin only as arguments of `make`, and an X4
image has no `.data`, `.bss`, TLS or imports (`abi/ppn5.md`). Rust's `#[global_allocator]` is called
without any argument, so it cannot reach heap slots 0–2: a Rust plugin that uses `alloc` (`Vec`,
`String`, `Box`, and the shared SDK crates such as text and layout) has no heap. Rewriting every crate
and all 70 ports without a global allocator is not possible before 1.0, and passing a context in a CPU
register (`tp`/`gp`) is platform-specific and fragile. So interface 7 gets exactly one read-only word
the loader fills before `make`.

## 2. The word

- [D] A plugin that allocates defines **one** pointer-sized, pointer-aligned, read-only object
  `PAPERPAW_HEAP` (global, default visibility), in the section `.rodata.paperpaw_heap`, holding **0**
  in the file. Before `make` the loader writes into it the address of the host's `PpHeap` table (§3).
  It is the third export besides `PAPERPAW_ENTRY` and `PAPERPAW_METADATA`, and it is optional: a plugin
  that never allocates leaves it out.
- [D] It is the **only** writable-by-the-loader word in a plugin. It holds no host table, no context,
  no plugin state: a plugin that wants any other host service still uses the `PpHost` and `ctx` it got
  from `make`. The rule of `abi/entry.md` ("no global host-pointer slot, no writable plugin static")
  stands with this single named exception.
- [D] The plugin reads it, never writes it. Its value never changes while the plugin is placed. A
  value of 0 (a host that did not fill it) means "no heap": every allocation fails (null), and the
  SDK's out-of-memory path runs (§6).
- [D] **Definition.** The SDK defines the word in assembly (`global_asm!`, emitted by the generator
  for each target profile) and declares it to Rust as `extern "C" { static PAPERPAW_HEAP: ... }`. Rust
  never sees an initial value, so no read is folded to 0 and no Rust static is written behind the
  compiler's back. (Measured with rustc 1.99.0: an `UnsafeCell` static in `.rodata.*` gets a *writable*
  section, which the packer refuses; a Rust `static X: usize = 0` is folded to 0 on plain reads. The
  assembly definition gives flags `A` and real loads on riscv32imc and x86_64.) C plugins use the
  generated header's `PP_DEFINE_HEAP_WORD()` macro and the `pp_heap()` accessor, which do the same.

## 3. The heap table `PpHeap`

```c
typedef struct PpHeap {
    uint32_t size;      /* bytes of this table the host provides */
    uint16_t major;     /* 7 */
    uint16_t minor;     /* the minor the table was built for */
    void    *(*alloc)(size_t size, size_t align);
    void     (*dealloc)(void *ptr, size_t size, size_t align);
    void    *(*realloc)(void *ptr, size_t old_size, size_t align, size_t new_size);
    _Noreturn void (*abort)(int32_t code, const uint8_t *msg, size_t msg_len);
} PpHeap;
```

- [D] Entries take **no context**. The table is host-owned, read-only for the plugin, and valid for
  as long as any code of the plugin may run. It grows like `PpHost`: entries are only appended, each
  with the minor it came with, and the plugin checks `size` before it reads an entry (generated
  checked accessors, as for `PpHost`; a missing entry is "no heap").
- [D] `alloc(size, align)`: a block of `size` bytes aligned to `align`, or **null**. `size` is not 0;
  `align` is a power of two at most `MAX_ALIGN` (4,096). Null for a zero size, a bad or too large
  alignment, a full budget, a full heap, and outside a dispatch frame (§4). Never aborts.
- [D] `dealloc(ptr, size, align)`: gives back a block of this heap with the size and alignment it was
  made with. Never fails and never needs a frame; a null `ptr` is ignored. A pointer this heap did not
  hand out, or a wrong size/alignment, is a plugin fault: the host may detect it and treat it as
  `abort` (§6), and must never corrupt another owner's memory because of it where it can tell.
- [D] `realloc(ptr, old_size, align, new_size)`: as Rust's `GlobalAlloc::realloc`. The block keeps
  its owner. Null leaves the old block untouched and still owned. Null outside a frame.
- [D] `abort(code, msg, msg_len)`: does not return. See §6.
- [D] **One allocator** (Senior 5). Heap slots 0–2 and this table are two doors to the same host
  allocator: one implementation, one block format, one per-plugin budget. A block from either door may
  be freed or grown through the other. Slots 0–2 take the owner from `ctx`; this table takes it from
  the current frame (§4). The SDK uses this table for its global allocator and nothing else; it does
  not offer a second allocation API.

## 4. Who pays: the frame owner (Senior 3, 4; Manager 3)

- [D] **Every entry from host code into plugin code pushes that plugin as the frame owner**, and
  restores the previous owner on every return path, errors and nested dispatch included (a guard
  value, not paired calls). That covers `make`, every `PpPlugin` callback (`drop`, draw, input,
  `dispatch_event` and those appended later), the teardown hooks (cancel, release, destroy), and every
  host-to-plugin callback a host slot runs (visitors, sinks). Core keeps the list of these call sites
  with a test for each kind; a new kind of entry is not accepted without its frame.
- [D] `alloc` and `realloc` charge the **frame owner**. Outside any frame they return null; they never
  charge "nobody" or the kernel.
- [D] `dealloc` and the owner kept by `realloc` follow the **block's owner**, recorded by the host
  (a block header or a per-owner arena, the host's choice; the plugin cannot observe it), never the
  frame. So a block made in one frame and freed in another, or freed in a frame of another plugin
  (a block handed over through the host, which interface 7 does not do, or a bug), leaves both budgets
  exact.
- [D] **Budget.** Each plugin has the per-plugin heap budget of the resource profile, shared by both
  doors. One past the budget is null (out of memory for the plugin), not a kernel-wide failure.
- [D] **Teardown** (Senior 6). After a plugin's `drop`/destroy returned, the host frees every block
  that plugin still owns and logs the count (a leak report, not an error). It does not free them while
  the owner is kept alive by the leak rule of #232 (a teardown that failed or was refused): then the
  blocks stay with the never-reused owner id and count against nothing else.

## 5. Placing the word: loaders (Senior 1, 2, 8; Manager 2)

### X4 (format 5)

- [D] `ppn5` header: **`flags` bit 0 `HEAP_WORD`** and the reserved word at 60..64 becomes
  **`heap_at`**, the image offset of `PAPERPAW_HEAP`. Bit 0 clear: `heap_at` is 0 and the plugin has no
  word. Every other flag bit stays "newer format".
- [D] The loader refuses the file (malformed, step 7 of `ppn5.md`) unless, with bit 0 set: `heap_at`
  is 4-aligned; `heap_at + 4 <= code_at` (in the read-only data); the word lies outside the entry
  (`[entry, entry + entry_bytes)`) and the metadata (`[meta_at, meta_at + meta_len)`); it is not a
  relocation word (no listed offset equals `heap_at`; both are 4-aligned, so equality is overlap);
  and it holds **0** in the file (step 9, read with the relocation words).
- [D] The signature and the header digest cover the file, where the word is 0; the loader writes the
  value only into the placed image, like a relocated word. The three digests of the write pass are
  unchanged: the word is not in the relocation list, and the image digest is taken of the bytes read,
  before relocation and before the word is written.
- [D] **The value is written at every placement and never outlives the firmware build** (Senior 2).
  The X4 already keys its flash layout by firmware build: the flash index stores the SHA-256 the
  firmware image ends with, and a different firmware ignores the index and erases, relocates and
  writes every plugin again (needed today for the build-specific addresses relocation writes). The
  loader writes the `PpHeap` address while it places the image, as it relocates, so a firmware
  change re-places the word with everything else. No fixed address joins the target profile. A
  loader that keeps placed images must key them by firmware build id in the same way.
- [D] `paperpaw-sign pack` finds `PAPERPAW_HEAP` by symbol, checks it is in `.rodata`, 4 bytes,
  4-aligned, 0, not relocated, and writes `heap_at` and bit 0; any other writable or loader-patched
  object stays refused. `PAPERPAW_HEAP` joins `PLUGIN_EXPORTS` as an optional export.

### Desktop and other hosted loaders

- [D] The loader looks the symbol up after `dlopen`, checks size (symbol table), alignment and that
  it lies in the module's read-only data, makes **that page** writable, writes the table address,
  restores read-only, then calls `make`. The table lives in the host process for the module's
  lifetime. Same frame rules as the X4.

## 6. Running out of memory and panics (Senior 7)

- [D] The SDK's global allocator returns null on refusal, so `try_reserve`-style SDK paths stay
  fallible. A failed infallible allocation (`Vec::push` at the limit) ends in
  `handle_alloc_error`, which panics.
- [D] The SDK's `#[panic_handler]` (on `target_os = "none"`) calls `PpHeap.abort` with a code and the
  first bytes of the message. A plugin panic is a **plugin fault**: on the X4 the host records the
  frame owner as the faulting plugin, switches it off for the next start (the X2 crash path), and
  resets the device. Outside any frame it is recorded without a plugin. On hosted targets the host logs
  the owner and ends the process. Never unwinds into the host.
- [D] Codes: 1 panic, 2 allocation failure (`handle_alloc_error`), 3 bad free detected by the host;
  others reserved.

## 7. Bookkeeping

- `abi.def`: `heap_table` (`PpHeap`, append-only like the host entries, lock-recorded), the
  `PAPERPAW_HEAP` export in `entry_contract`, `MAX_ALIGN`, the ppn5 `flags` bit and `heap_at` field;
  generated Rust, C header (with the macro and checked accessors) and JSON.
- SDK: `native_plugin!` defines the word and installs the global allocator and panic handler; a
  plugin that wants no heap opts out. Shape change (new export and table), so the next draft, one
  blessing line, CHANGELOG.
- Tests: allocator door equivalence (alloc through the table, free through slot 1, and back); null
  outside a frame; frame restore after an error return and after nested dispatch; budget refusal per
  plugin; teardown frees and counts leftovers; the leak rule keeps blocks; X4 loader refusals for each
  check in §5 with fixture files (word at an odd offset, in code, in the entry, in the metadata, on a
  relocation word, nonzero); a patched word in the placed image; desktop write-and-protect.

## 8. Amendment to ADR 0034 (sections 2, 3 and 10)

ADR 0034 first had a loader-filled `PAPERPAW_HOST` word pointing at the whole host table;
`abi/entry.md` and `abi/ppn5.md` removed it in favour of `make` arguments only. This amendment brings
back **one narrower word**, `PAPERPAW_HEAP`, that points at a context-free heap table and nothing else.

- **Why:** a language-level global allocator (Rust's `#[global_allocator]`, C's `malloc` shim) is
  called without arguments; with no writable static and no imports, nothing else can reach the host
  heap, and without a heap no realistic Rust plugin or SDK crate can be built.
- **What stays:** the host table and every service still reach the plugin only through `make`; the
  plugin holds no global host pointer. The heap word gives allocation and abort, nothing more, and
  every allocation is charged to the plugin whose frame is current.
- **Rejected:** (B) SDK collections without a global allocator (rewrite of every crate and all ports);
  (C) a context in a register (`tp`/`gp`: platform-specific, clobbered by C code and interrupts).
- **Section 2 "Heap"** keeps its rule that nothing allocated by one side is freed by the other; with
  this amendment the plugin's global allocator calls the host's heap through the word, as section 2
  already assumed.
