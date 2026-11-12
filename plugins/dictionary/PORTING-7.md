# Dictionary on interface 7 (slices 1 to 3, partly ported)

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
  the first builder and declares the work again until all are made. An index that cannot be made (or a refused allocation while making it) ends
  the work and is not tried again until the next start (interface 6 kept a `failed` file with the
  files' sizes: not ported yet; a restart tries again).
- Metadata: services `core.files`, `core.work`, feature `uses.storage`, the `storage.*` and
  `services.query` calls.
- Tests: `tests/native_source.rs` (indexes made by the builders in steps, a word looked up, data and user
  files kept apart, a broken dictionary left out, an unlistable folder an error).
- Not done: the folder of caches of removed dictionaries is not cleaned (`forget_others`); host
  acceptance (the lookup broker, the host running `work_due`) stays in `tests/acceptance_pending.rs`.

## Slice 3: the panel (`src/native_panel.rs`, `src/native_popup.rs`)

- The screen `dictionary.popup` (an overlay, token 22) fills the slot `core.lookup`
  (`slot_offer`); the host admits both (`tests/native_declarations.rs`). It takes `word` and
  `language` from its hand-over.
- `native_panel::Panel` (host-free, `native_panel/tests.rs`): the entries as one text, the source's
  name above each where there are several (drawn faint), wrapped by words into at most 600 lines,
  a page at a time; buttons scroll (next/previous page, up/down), confirm and back close; touch:
  the right part scrolls on, the left part goes back (or closes on the first page), outside closes.
- `native_popup::View`: asks the providers through the `core.lookup` service (one page, so one entry,
  per provider, up to 16), draws the lower 55 % of the screen (word, page note, lines) through the
  kit's paint, scrolls with a partial redraw, closes with `close_screen`.
- Metadata: services `core.lookup`, `core.ui` added.
- Not ported (each stays an ignored case or a named gap): *Online ›* in the header and entries
  that open more (*Open Wikipedia ›*): the host answers a provider that must fetch as unsupported
  today and has no pending-ask flow for plugins that ask (`document.md` 7, todo 98); the line that
  says the indexes are being prepared (the provider answers once they are ready, the panel shows
  *No entry* meanwhile); several definitions of one word in one dictionary or of several
  dictionaries: the service gives one entry per provider page, cut at 2,048 bytes (interface 6
  showed every definition); the text of the *No dictionaries* message (`dictionary.none`) is not
  shown (the panel says *No entry*); the hint of the confirm button (*Close*); goldens (the host
  draws no native screen yet: H1/H3/H4).

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

## Slice 4 (#257): the panel reads the own dictionaries itself, whole

- The lookup service skips the asker's own provider, so the panel used to show nothing of the plugin's own
  dictionaries. `native_popup::read_own` now reads them for the panel through `Dictionaries` (the same source as
  the provider): every definition, **whole** (the 2,048-byte entry limit is the service's, for what other plugins are
  given), in dictionary order, within one shared budget of `paperpaw_stardict::LOOKUP_BUDGET` (32 KiB, what a
  lookup reads; Beta 3 gave each dictionary its own 32 KiB). The providers of other plugins follow, one page each,
  as before. The lines kept at most are 1,200 (8 bytes each on the device), the text is built in one allocation and
  the entries are dropped once the panel holds the wrapped text.
- A folder with no `.ifo` file, where no other provider answered either, shows *No dictionaries in "<folder>"…*
  (Beta 3 showed it when no other provider fitted at all; the native panel cannot know the providers' languages
  beforehand, so it says it only when nothing at all was found).
- Dictionaries whose indexes are not made are counted (so the message is not shown) and read after the work made
  them; the *Preparing* note comes in the next slice.
- Tests: `tests/native_panel_own.rs` (a 5,000-byte definition whole, the budget, not ready, unlistable folder),
  `native_model` and `native_panel` unit tests, allocation refusal of `all_within`.

## Slice 5 (#257): preparing, the failed marker, the caches of removed dictionaries

- **Preparing.** The plugin keeps the indexes it makes (`Index`: the builders, bytes done and in all). A lookup (the
  provider) or an opened panel that finds dictionaries without indexes hands them to it and declares the work
  `dictionary.index`; each `work_due` does one 64 KiB step, as before. The panel waits: its header says *Preparing … N %*
  (`dictionary.preparing`), the body is empty, back, confirm and a tap close it. When the last index is made the
  plugin tells the open panel, which reads the word again (own dictionaries, then the providers) and draws.
  Where indexing was given up this session (an error that may pass), a new panel shows what is ready.
- **`failed` marker.** A damaged dictionary (`paperpaw_stardict::Error::Malformed`) is left out, and the file
  `<cache>/failed` in the data folder keeps the sizes of the files that were to be indexed (`native_disk::stamp`).
  `Dictionaries::pending` leaves out a dictionary whose marker equals its stamp: it is tried again only when the files
  change (as Beta 3). Other failures (a card that could not be read, a refused allocation) end the indexing for the
  session only.
- **Caches of removed dictionaries.** When a panel lists the folder, the folders of 16 hexadecimal digits in the data
  folder that no dictionary found names are removed with `core.files` (`Folders::forget_caches`, root 0,
  `remove_tree`); best effort, never when the folder itself cannot be listed.
- Tests: `tests/native_source.rs` (the marker), `native_disk` (`is_stale_cache`), `native_plugin` (the percent).
  Not run on a host (the panel's wait, the work steps and the removal need a scripted host with `core.ui` and
  `core.files`: `tests/acceptance_pending.rs`).

## Slice 6 (#257): *Online ›*

- Where a screen is offered for the slot `core.lookup-online` (Wikipedia's; `native_online::offered`, the registry's
  `slot_offers`, checked when the panel opens), the header says *Online ›* left of the page note, as Beta 3. A tap on
  it, or the menu button (`core.menu`), closes the panel and opens the slot with a hand-over of the word and the
  language (`native_online::go`; Beta 3: back, then `OpenSlot`). Not shown while the panel prepares indexes.
  The service `core.registry` joins the metadata (regenerated).
- Beta 3 asked network providers (providers that fetch, flag `online`) only on *Online ›* and showed their entries in the
  panel, and its entries could end with a line that opens more (*Open Wikipedia ›*). The native lookup service calls a
  provider that fetches itself whenever the network is up, and an entry has no "opens more": the line is replaced by
  *Online ›* (the same screen), and no first-party provider that fetches exists (Wikipedia is the slot's screen).
- Not done: the hint text of the menu button (*Online ›*; the host's hints, #892) and *Looking up …* while providers
  answer later (todo 98).
- Tests: `native_online` (what asks to go online), `native_popup` (the place of the label); not run on a host (the
  slot's offer and the opening need a scripted host with `core.registry` and `core.ui`).

## What the slice loses (each returns in a later slice or with its host piece)

- **Every lookup** (done in slice 2 for the provider).
- The panel (definitions, scrolling, the *Online* line, *Open more*), and the slot `core.lookup` it
  fills: a screen that cannot draw is not declared.
- Background indexing of a dictionary's blocks on the first lookup (done in slice 2, as the provider's work).
- Other plugins' providers in the panel, and providers answering later (todo 98).
- The setting's section heading (`dictionary.section`).
- Entries are cut at 2,048 bytes (interface 6 scrolled the whole definition).

Each loss is an ignored case in `tests/acceptance_pending.rs`.
