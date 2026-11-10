# Dictionary on interface 7 (native only, with named gaps)

Status: **native only.** The interface-6 plugin (registration, panel, background indexing, providers
list), its kernel tests (`lookup*.rs`, `providers.rs`) and its registration in the app are removed; the
Android shell names the folder setting (`dictionary.folder`) itself. Version stays 1.0.3 (Beta 3 is 1.0.2:
no further bump).

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

## Gaps against Beta 3 (each is an ignored case in `tests/acceptance_pending.rs` or named here)

- Providers that fetch (*Online ›* in the header, *Open Wikipedia ›* on an entry, providers answering
  later): the host answers them as unsupported (todo 98, on the activation blocker list).
- The line that says the indexes are being prepared: the panel shows *No entry* meanwhile.
- Several definitions of one word in one dictionary or of several: the service gives one entry per provider
  page, cut at 2,048 bytes (interface 6 scrolled every definition).
- `dictionary.none` (the *No dictionaries* message) is not shown; the setting's section heading
  (`dictionary.section`); the hint of the confirm button; the clean-up of the caches of removed
  dictionaries (`forget_others`) and the `failed` marker of an index that could not be made (a restart
  tries again).
- Goldens: the host draws no native screen yet (H1/H3/H4); no snapshot is blessed.
- Not in the app's `NATIVE_PLUGINS`: like the other native-only plugins it joins the program at activation
  (Core #944); until then no dictionary runs in the desktop or Android program.
- **Activation checklist (`NATIVE_PLUGINS`; this PR is HELD until native activation).** In the step that
  activates native plugins: (1) list `NativeDictionary` in `paperpaw_app::NATIVE_PLUGINS` (keep the array length
  and the counts in `crates/paperpaw-app/tests/sets.rs` equal) and in `tools/plugin-files7/complete.txt`;
  (2) check that the shells' dictionary folder setting still uses the plugin's id `dictionary.folder` (Android's
  `DICTIONARY_FOLDER` repeats the literal; a shared constant is not exported yet); (3) bless the dictionary
  goldens once H1/H3/H4 draw the screens.
- A dictionary that cannot be read is skipped at every lookup, and an index that cannot be made is tried
  again at every start (interface 6 kept a `failed` marker with the files' sizes).

