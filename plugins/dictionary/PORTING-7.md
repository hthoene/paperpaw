# Dictionary on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (registration, panel, background indexing, providers
list) is still in place beside the native one. It is removed in the PR that finishes the port.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`, `src/native_entry.rs`)

- Declarations: the folder setting (with the folder chooser) and the StarDict dictionaries as a
  lookup provider (`core.lookup`, offline, every language). `build.rs` and `native/metadata*` are
  generated with `tools/abi-metadata.py`. En/de texts are the existing ones.
- Model (host-free): finding `.ifo` files below a folder (three folders deep, hidden names left
  out, sorted, at most 64), the entry a lookup page asks for among the definitions of all ready
  dictionaries (a dictionary that cannot be read is skipped, `NoMemory` is not), and cutting an
  entry to the contract's limits (title 256, text 2,048 bytes, at a character boundary).
  Everything allocates fallibly; `tests/native_alloc_refusal.rs` refuses each allocation.
- The provider reads a stage call (`Ask`), picks the page by its cursor and answers one entry with
  `more` set when another follows. It has no dictionary to read from yet, so every ask is
  answered `NotFound` (no entry): the provider is declared but silent until the files slice.

## What the slice loses (each returns in a later slice or with its host piece)

- **Every lookup**: dictionaries cannot be read until `core.files` (#384/#339) and StarDict on native
  files exist (`paperpaw-stardict` reads through interface 6's `Storage`).
- The panel (definitions, scrolling, the *Online* line, *Open more*), and the slot `core.lookup` it
  fills: a screen that cannot draw is not declared.
- Background indexing of a dictionary's blocks on the first lookup (needs the work SDK).
- Other plugins' providers in the panel, and providers answering later (todo 98).
- The setting's section heading (`dictionary.section`).
- Entries are cut at 2,048 bytes (interface 6 scrolled the whole definition).

Each loss is an ignored case in `tests/acceptance_pending.rs`.
