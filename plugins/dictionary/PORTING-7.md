# Dictionary on interface 7 (slices 1 and 2, partly ported)

Status: **not complete.** The interface-6 plugin (registration, panel, background indexing, providers
list) is still in place beside the native one. It is removed in the PR that finishes the port.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`, `src/native_entry.rs`)

- Declarations: the folder setting (with the folder chooser) and the StarDict dictionaries as a
  lookup provider (`core.lookup`, offline, every language). `build.rs` and `native/metadata*` are
  generated with `tools/abi-metadata.py`. En/de texts are the existing ones.
- Model (host-free): finding `.ifo` files below a folder (three folders deep, hidden names left
  out, sorted, the first 64 in that order), the entry a lookup page asks for among the definitions of all ready
  dictionaries (a dictionary that cannot be read is skipped, `NoMemory` is not), and cutting an
  entry to the contract's limits (title 256, text 2,048 bytes, at a character boundary).
  Everything allocates fallibly; `tests/native_alloc_refusal.rs` refuses each allocation.
- The provider reads a stage call (`Ask`), picks the page by its cursor and answers one entry with
  `more` set when another follows. It has no dictionary to read from yet, so every ask is
  answered `NotFound` (no entry): the provider is declared but silent until the files slice.

## Slice 2: the provider reads the dictionaries (`src/native_disk.rs`, `native_folders.rs`, `native_source.rs`)

- Version stays 1.0.3 (above Beta 3's 1.0.2: no further bump).
- `native_folders::Folders` lists the dictionary folder through `core.files` (root 1, pages of the
  service, hidden and unaddressable names skipped); `native_disk::HostRoot` reads root 1 and the data
  folder through `storage`; `Routed` makes them one `Storage` for the StarDict reader: a path that starts
  with `CACHE` (a character no card path can hold) is the data folder, which holds the indexes
  (`<16 hex of the ifo path's hash>/idx.qbi`, `syn.qbi`); the user's folder is never written.
- `native_source::Dictionaries` is the provider's `Source`: found with `find_dictionaries`, opened with
  `paperpaw-stardict`; a dictionary that cannot be read is left out, `NoMemory` is not; one whose
  indexes are not made is not ready and its `Builder`s are returned by `builders()`.
- `NativeDictionary::event` answers a `core.lookup` stage call from the folder setting (default
  `dictionaries`) and, when indexes are missing, keeps their builders and declares the work
  `dictionary.index` (`core.work`, event `WorkDue`, token 21); each `work_due` does one 64 KiB step of
  the first builder and declares the work again until all are made. An index that cannot be made ends
  the work and is not tried again until the next start (interface 6 kept a `failed` file with the
  files' sizes: not ported yet; a restart tries again).
- Metadata: services `core.files`, `core.work`, feature `uses.storage`, the `storage.*` and
  `services.query` calls.
- Tests: `tests/native_source.rs` (indexes made by the builders in steps, a word looked up, data and user
  files kept apart, a broken dictionary left out, an unlistable folder an error).
- Not done: the folder of caches of removed dictionaries is not cleaned (`forget_others`); host
  acceptance (the lookup broker, the host running `work_due`) stays in `tests/acceptance_pending.rs`.

## Notes for the files slice (done in slice 2; kept for the record)

- The caller trims the folder setting's leading and trailing `/` before `find_dictionaries`
  (interface 6 did: `folder.trim_matches('/')`).
- The `Source` bounds a definition's size when it reads from the dictionary (`definitions` allocates
  what the file holds); the model cuts only at the entry's limits.
- "No entry" is `NotFound` from the stage call; whether the host reads that as no row is Kernel's
  to confirm (document.md section 7).

- The broker turns a provider's "no entry" into an empty `lookup_entry` (Kernel's ruling, todo 123):
  the panel (slice 2) and the reader's selection menu skip pages for which
  `native_model::is_no_entry` is true.

## What the slice loses (each returns in a later slice or with its host piece)

- **Every lookup** (done in slice 2 for the provider).
- The panel (definitions, scrolling, the *Online* line, *Open more*), and the slot `core.lookup` it
  fills: a screen that cannot draw is not declared.
- Background indexing of a dictionary's blocks on the first lookup (done in slice 2, as the provider's work).
- Other plugins' providers in the panel, and providers answering later (todo 98).
- The setting's section heading (`dictionary.section`).
- Entries are cut at 2,048 bytes (interface 6 scrolled the whole definition).

Each loss is an ignored case in `tests/acceptance_pending.rs`.
