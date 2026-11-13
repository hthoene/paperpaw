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
- [D] **The plugin reads a hidden alias.** The assembly also defines a local `.hidden` alias at the
  same address, and the plugin's code reads only that alias; the default-visibility name exists for
  the loader and the packer. On hosted targets a default-visibility symbol read from inside a shared
  object goes through the GOT and could be interposed by a host process that exports the same name;
  the alias cannot be. The linker scripts (`paperpaw-plugin7.x` and the hosted one) `KEEP` the
  section, so a plugin that defines the word never loses it to section garbage collection.

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
  with the minor it came with. The plugin reads entries only through the generated checked readers
  (`read_heap_alloc` and the others, from the same generator as the `PpPlugin` readers): each checks
  that the table is aligned, its `major` is 7 and its `size` covers the entry before it reads it,
  and gives "no heap" otherwise, never a read past `size`. Nothing is cached (a plugin has no
  writable static to cache in; the checks are two loads).
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
  value, not paired calls). The entry kinds of interface 7:
  1. `make` (the entry's factory);
  2. `PpPlugin.drop`;
  3. `PpPlugin.draw`, `PpPlugin.input`, `PpPlugin.dispatch_event`, and every callback appended to
     `PpPlugin` later;
  4. the teardown hooks `cancel`, `release` and `destroy` (the host's `report` runs no plugin code);
  5. every host-to-plugin callback a host slot runs while it serves the plugin (visitors, sinks).
  All of them go through one host function in `native_host` that pushes the frame (Core's); a test
  per kind shows the owner inside the call and the previous owner after it, after an error return
  and after nested dispatch. A new kind of entry is not accepted without its frame and its test.
- [D] `alloc` and `realloc` charge the **frame owner**. Outside any frame they return null; they never
  charge "nobody" or the kernel.
- [D] `dealloc` and the owner kept by `realloc` follow the **block's owner**, recorded by the host
  (a block header or a per-owner arena, the host's choice; the plugin cannot observe it), never the
  frame. So a block made in one frame and freed in another, or freed in a frame of another plugin
  (a block handed over through the host, which interface 7 does not do, or a bug), leaves both budgets
  exact.
- [D] **Budget.** Each plugin has the per-plugin heap budget of the resource profile, shared by both
  doors. One past the budget is null (out of memory for the plugin), not a kernel-wide failure.
  The padding an alignment needs (up to `MAX_ALIGN`) and the host's per-block bookkeeping count
  against the plugin's budget too.
- [D] **Bookkeeping on the X4.** The X4's heap cannot list its blocks, so the teardown sweep needs a
  per-block header with the owner and a list link (or a per-owner arena); its bytes are the
  bookkeeping above.
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
  change re-places the word with everything else. No fixed address joins the target profile: the
  re-placement after a firmware change is already required by the other build-specific relocations,
  so a fixed address would save no flash write or start time and would add a frozen promise. A
  loader that keeps placed images must key them by firmware build id in the same way.
- [D] The X4 loader writes the word in the same flash pass as the rest of its sector (no second
  erase or write for it): NOR flash only clears bits, so the loader stores the address into the
  piece buffer after relocating that piece and before the sector is programmed. A placed word is
  never rewritten in place. A format-5 file without `HEAP_WORD` stays valid: such a plugin has no heap.
- [D] **Checked before every run.** On every start, before `make`, the loader compares the placed
  word with the address of its current `PpHeap` table. On a mismatch it never runs the plugin: the
  X4 refuses it as damaged (the build-keyed index already re-placed everything it would re-place,
  so a mismatch means a damaged or foreign region); a hosted loader may place it again. Any later check of placed bytes treats the word like a relocated
  word.
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
  the owner and ends the process, the Workshop emulator included, so the Workshop can start it again
  and name the plugin. Never unwinds into the host.
- [D] **No usable heap word.** Where the word is 0, the table's `size` does not cover `abort`, or
  the word is missing, the panic handler has nowhere to report: it executes the profile's trap
  instruction (`unimp` on RISC-V, `ill` on Xtensa, `ud2` on x86_64). The X4's crash slot then
  blames the frame owner as for any fault. A test builds a plugin whose word is 0 and shows the trap
  path is taken.
- [D] **No re-entry by formatting.** The handler copies a literal message (`as_str`) without
  formatting; only a message with arguments is formatted, which runs the plugin's own `Display`
  code. A panic inside that code re-enters the handler and recurses until the stack overflows (no
  writable static can guard it). That remaining case must end as a plugin fault. **On the X4 this is
  not proven today** (Firmware, from code): plugin frames run on the shell's main stack, whose only
  hardware net is one 4-byte watchpoint word that a sparse frame can step over, and the shell's
  panic handler has no re-entrancy guard. Activation gate for interface-7 plugins on the X4 (the
  heap word and every allocating port included): the shell arms a CPU store trigger on a 2 KiB
  aligned band at the main stack's end, which fires in the overflowing store (so the faulting code
  is blamed) and keeps `RESERVE` (below) for the exception handler and the crash slot; a
  re-entrancy flag in its panic handler (a second entry writes a fixed "panic in panic" slot
  blaming the frame owner and resets, without formatting again); and an owner-approved device test
  that recurses until overflow shows the crash slot blames the plugin. (A stack-pointer monitor is
  not usable: the radio tasks run on other stacks.)
  **Frame limit (decision #953).** The band fires only on a store. One frame of size F can move
  the stack pointer into the band without storing, and the next frame then stores up to F lower,
  so the exception handler and the crash slot have only `RESERVE + 4 - F` left. Rules, with `BAND`
  the armed band, `H` the handler's measured depth and `M` a margin of 256 bytes:
  - `LIMIT <= BAND`: no frame is larger than the band, so a frame that starts above it cannot skip
    it. On the X4 `LIMIT` is 2,048 bytes, equal to `BAND`.
  - `RESERVE = H + M + LIMIT`: the stack kept below the band's top for the handler, so the
    two-frame case still leaves the handler its depth. Until `H` is measured on the device the X4
    keeps 4,096 bytes, and this gate stays open.
  - The pack-time check (`paperpaw-sign`, #252) refuses any plugin frame above `LIMIT`. Firmware
    functions that plugins call on the plugin's stack (shared-table entries, host services, the
    dispatch entries after #235) count against `LIMIT` too; their budget is checked in #254.
  - Firmware sets the constants in #250 and #252.
  Hosted targets end the process on the overflow.
- [D] **The ESP32-S3's band (X4 Pro, X4 Classic; decision 142, draft 29).** The S3 has neither
  the X4's store trigger over a 2 KiB region (its data breakpoints cover at most 64 bytes, and
  esp-hal and esp-rtos already use one for the task guard words) nor a stack-pointer monitor (the
  debug assistant's SP monitor is absent on the S3; it would also see the radio tasks' stacks).
  The band is the **debug assistant's region monitor** instead: `ASSIST_DEBUG` DRAM0 area 0, write
  monitoring, on the core that runs plugin calls (core 1 too where started). It watches any range,
  sees every CPU store into it, the window spills' `s32e` included, and never sees another task's
  stack. Rejected: the 64-byte watchpoint (any frame over 64 bytes steps over it), the PMS/World
  Controller (block and split-line granularity, built for isolation, not a band), and a canary with
  a headroom check (after the fact; a headroom check at entry bounds nothing the plugin does after
  it). The paint check (`stack::check`) and esp-hal's guard word stay as they are.
  - **It is an interrupt, not an exception.** The store into the band completes and the interrupt
    follows a few instructions later; the band is spare stack, so that store harms nothing. It runs
    at the highest level esp-hal lets Rust handle (`Priority3`). The host never runs plugin code at
    an interrupt level of 3 or more: the dispatch entry reads `PS.INTLEVEL` and refuses the call
    when it is not 0 (logged once, the plugin is not run), and the packer refuses plugin code that
    can raise it (below). The handler first disables the monitor, then panics like the X4's band
    (crash slot, frame owner blamed).
  - **Window spills (Xtensa only).** A windowed function stores nothing when it is entered: `entry`
    only moves `a1`, and the return address stays in a register. A frame's own stores lie at or
    above its `sp - 16` (the 16-byte base save area below each `sp` holds its caller's `a0`-`a3`
    once spilled; the call8 extra save area lies inside the caller's frame). So a chain of frames
    can walk `sp` through the band with no store, and the store that catches it is a spill: the
    caller of the first frame whose `sp` lies in the band spills into that frame's base save area,
    inside the band. Spills happen at the latest when the register file overflows, and at every
    interrupt or exception (xtensa-lx-rt spills all windows on entry). With 64 address registers
    and call8 (a rotation of 8), at most 7 windows are live; the caller and the frame in the band
    take 2, so at most **6 frames** run below the band before the spill, each under `LIMIT`.
  - **Rules:** `LIMIT` = 2,048 bytes as on the X4, counted as the `entry` operand plus the 16-byte
    base save area. `BAND` = 2,048 bytes, 16-aligned (no power-of-two alignment needed): every frame
    moves `sp` by less than `BAND - 16`, so some frame's `sp` lands in `[band + 16, band + BAND]`
    and its base save area lies in the band. `RESERVE = 6 * LIMIT + DEPTH`, with `DEPTH` the larger
    of the handler's and an interrupt's measured depth plus 256 bytes (2,048 assumed, the S3's
    exception frame being 256 bytes plus the spills): 14,336 bytes between the stack's end and the
    band, 16 KiB with the band, before the masked depth `CRIT` (below) is added. The S3's
    `STACK_FLOOR` grows by that over the measured deepest path (some 33 KiB): about 50 KiB, from
    the internal heap (`HEAP_SIZE`); PSRAM keeps the rest.
  - **Firmware on the chain (Senior M1).** The six-frame bound holds for the whole call chain,
    not only the plugin's code: a plugin calls into the firmware (host slots, shared-table
    entries, the dispatch entries), and those frames run on the same stack below the band. So on
    the S3, as on the X4, every firmware function a plugin's call can reach keeps its frame
    (`entry` operand plus 16) under `LIMIT`, and the whole chain calls with `call8` or `call12`
    only: a `call4` chain (ROM or C code compiled for it, reached through a host slot) fits up to
    14 windows in the register file and could run 14 frames below the band before the spill. The
    S3 firmware build runs the same frame and call-width scan over every function reachable from
    the host table and the shared table (Firmware's check, `frame-budget-ppn5.txt`); the band is
    armed, and `band_armed()` can be true, only in a build where that check passed. A firmware
    function that fails it is fixed (smaller frame, `call8`), never waived; a ROM function called
    with `call4` is wrapped or not reachable from a host slot.
  - **Masked delivery (Senior S1 on #917).** A store into the band while level 3 is masked (inside
    a critical section of the firmware: esp-alloc, flash and PSRAM work, `println`; or anywhere at
    `INTLEVEL` 3 or more) only latches the monitor's raw bit: the interrupt comes when the mask
    ends, and until then the masked path can go deeper. Plugin code cannot mask (packer, below),
    but its calls reach masked firmware paths (every allocation takes esp-alloc's section). So
    `CRIT`, the deepest stack any masked section reachable from a plugin's call uses below the
    `sp` it was entered with, counts into the reserve: `RESERVE = 6 * LIMIT + DEPTH + CRIT`. The
    S3 frame scan reports `CRIT` over the masked paths it reaches (`frames-s3`: a function that
    masks counts its own and its callees' frames until the mask ends) and the band is armed only
    when the reserve in the build covers it; a masked path that cannot be bounded is a scan error,
    not an allow line.
  - **Packer, target 9:** refuses in plugin code `call0`, `callx0`, `call4` and `callx4` (more
    windows per register file; `call8` and `call12` stay); any write to `a1` but `entry`'s
    (`movsp`, `mov`, `addi`, loads into `a1`); and the instructions that change the interrupt level
    or the window state: `rsil`, `wsr`, `xsr`, `rotw`, `s32e`, `l32e`, `waiti` and every `rf*`
    return-from-exception. `rsr`, `rur` and `wur` stay (no ring or level is changed by them).
  - **Armed:** `stack::band_armed()` is true on the S3 only after the monitor's bounds and enables
    read back as set and a self-test at start passed: with its interrupt not yet enabled, one store
    into the band sets the monitor's raw bit, which is then cleared and the interrupt enabled. The
    loader refuses every format-5 file while it is false, as on the X4.
  - **Device checklist (S3), owner-approved tests:** S1 the monitor fires on a plain store and on a
    window-overflow `s32e` into the band, both bounds' inclusiveness, and the store-to-handler
    latency; S2 the start self-test passes on the X4 Pro and the X4 Classic; S3 `HANDLER` and
    `INTERRUPT` measured (the stack note, as G7); S4 the overflow test: a test plugin descends with
    `call8` frames just under `LIMIT` that store nothing, and the crash slot blames it with the
    paint below the reserve intact, once with Wi-Fi traffic arriving at the band; S5 no false
    firing in an hour of reading with Wi-Fi and Bluetooth on; S6 the deepest legitimate path on
    the S3 fits above the band (the stack note's `left`); S7 the 16 KiB of band and reserve come
    out of the S3's internal RAM (`STACK_FLOOR` about 50 KiB, `HEAP_SIZE` less by as much): the
    X4 Pro and X4 Classic start, connect to Wi-Fi (and Bluetooth on the Pro) and open a large book
    with the smaller internal heap, its low-water mark logged; S8 a store into the band from inside
    a masked section (an allocation at the band's edge) fires when the mask ends, before the stack
    passes the reserve's floor, and the measured masked depth stays within the scan's `CRIT`.
- [D] Codes: 1 panic, 2 allocation failure (`handle_alloc_error`), 3 bad free detected by the host;
  others reserved. A block grown by `realloc` is never reported as a bad free.

## 7. Bookkeeping

- `abi.def`: `entry_contract.heap_word` (symbol, hidden alias, section, table) and the `PpHeap`
  table after `PpEntry` (append-only like `PpPlugin`, lock-recorded); generated Rust (the table, the
  names and the checked `read_heap_*` readers), the C header (`PP_DEFINE_HEAP_WORD()` and the
  checked `pp_heap()`) and JSON. The metadata record is unchanged (its exports stay the entry): the
  `.ppn` header says where the word is, and hosted loaders look it up by name. The ppn5 `flags` bit
  and `heap_at` field are Firmware's header reader (`paperpaw-native` `header5`).
- SDK: `native_plugin!` defines the word and installs the global allocator and panic handler; a
  plugin that wants no heap opts out. Shape change (new export and table): the PR carries the
  regenerated interface files and a changelog fragment.
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

## 9. Statically linked built-ins: the host's part per plugin (todo 144, Senior on #496)

A built-in linked into the firmware (`native_plugin!(…, linked = NAME)`,
`Kernel::register_linked`) costs the X4 heap twice: the plugin's own state, which its `make`
allocates and its frame owner pays (section 4), and the host's part, which the kernel keeps for
as long as the plugin is registered. This section budgets the host's part.

- **One host table for all plugins.** The host table is a static per services type
  (`shell::SharedTable`, evaluated at compile time, in read-only data). Every hosted plugin's
  context holds a reference to it; none holds a copy. The table is 152 bytes on the X4; the
  reference that replaces the copy is 4, so this saves 148 bytes per plugin (1,332 bytes for 9
  built-ins).
- **Measured on riscv32** (compile-time pins in `native_host/shell.rs`): the host table is 152
  bytes, a plugin's context 168 bytes and its boxed owner 200 bytes (348 bytes with the copied
  table). `OWNER_BUDGET` is 224 bytes; a field that grows the owner past it fails the X4 build.
- **The rest of the host's part** is the plugin's id, its owner resources, its row in the native
  instance table and its declarations (screens, widgets, settings, menu and icon records). The
  unit test `hosted_owners_share_one_host_table_and_cost_a_bounded_heap` measures the whole part
  for a test plugin with one screen and one action: 1,906 bytes on x86_64 (2,194 with the copied
  table), bounded at 2,048 so that growth shows in review. That number is not a built-in's: the
  declarations grow with the plugin, and a real built-in declares many more records than the test.
- **Declarations are copied unless they lie in the image.** Staging keeps a submitted batch that
  lies wholly inside the plugin's read-only image as offsets and copies every other batch to the
  heap (`declaration_store`). A card plugin's image is its mapped `.ppn`; a linked plugin has no
  image range today (`linked_entry::hosted` passes an empty one), and the SDK's `Declarations`
  builder encodes into a heap buffer. So every linked built-in currently copies all of its
  declarations to the heap, and that copy, not the context, is the bulk of its host part.

**Rule 1: `make` runs once, at registration; no lazy start.** Declarations reach the host only
through `registrations.submit` while `make` runs (`Hosted::make_registered`), so the host cannot
register a plugin without making it. Registering by making and then dropping the plugin, to make
it again on first use, is refused: a second `make` loses whatever the first one set up (state,
storage reads, scheduled work) and may submit different declarations than the ones published,
and a `make` that runs late, inside a callback, can run out of memory where no caller can retry
it. A built-in is therefore made exactly once, when it is registered, and stays made while it is
registered, like a card plugin. Registering a plugin without `make` would need its declarations
outside `make` (in the entry or its metadata): that is a contract change and is not part of
interface 7 before 1.0.

**Rule 2: a linked built-in's declarations stay in flash.** The heap cost is cut where it is,
in the declaration copy:

1. `register_linked` takes the firmware's read-only data range from the shell (`&'static [u8]`,
   the linker's bounds of the flash-mapped read-only data on the X4; empty on shells without the
   constraint) and passes it to staging as the image, so a batch inside it is kept as offsets.
   The shell's safety contract already requires that range to stay mapped and unchanged for the
   program's lifetime. No interface change: `registrations.submit` already accepts static
   batches ("static declaration images can submit their encoded batch here").
2. Each built-in submits its declarations as static, already encoded batches (`static` byte
   data, at most `BATCH_MAX_BYTES` each), not through the runtime builder. A test of the plugin
   rebuilds the batches with `Declarations` and compares them byte for byte (blessed with
   `PAPERPAW_BLESS=1`), so the static bytes cannot drift from the builder or the codec. Card
   plugins may use the same pattern; they then also keep their declarations as offsets.
3. **Measured per built-in:** a host test registers each of the built-ins in `NATIVE_BUILTINS`
   through `register_linked` with a counting allocator and pins its host part (owner, id,
   resources, row, declaration index) with its own bound; riscv32 sizes come from the X4 build.

**Budget:** all built-ins together keep at most 4,096 bytes of host heap on the X4 while
registered, before their own state (section 4). The X4 shell activates a built-in only once the
measurement of rule 2.3 shows that the activated set fits; the measured numbers, not estimates,
decide how many built-ins the X4 starts.
