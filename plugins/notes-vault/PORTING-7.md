# Notes vault on interface 7 (slices 1 and 2, partly ported)

Status: **not complete.** The interface-6 plugin (the Markdown format, the vault's page, lists and
tags screens, the index, the home widgets, the registration) is still in place beside the native
one; the screens are native since slice 2, the format and the widgets are not. It is removed in the PR that finishes the port.

## Ported in slice 1 (`declarations.rs`, `src/native_model/`, `src/native_plugin.rs`)

- Declarations: the settings group (`notes-vault.group`, title `notes-vault.title`) and the one
  setting, the vault's folder (text, empty, help text, the folder chooser `core.choose-folder` as
  its editor). `build.rs` and `native/metadata*` are generated with `tools/abi-metadata.py`. En/de
  texts are the existing ones (`tests/native_translations.rs`).
- Model (host-free, over `Storage`, each part the interface-6 code made fallible and tested to give
  the same answers on fixed cases and on generated text and vaults, `src/native_parity_tests.rs`):
  - `lines`: a file a line at a time (one reservation for the chunk and the line, lines cut at
    1,024 bytes, bad bytes replaced as `from_utf8_lossy` does).
  - `note`: names, titles, folders, the excerpt (front matter skipped by looking, not by holding
    its lines; the unclosed case is a loop, not a recursion that a hostile `---` file could
    deepen), the excerpt of a file's first 2 KiB.
  - `tags`: the tags of a note's first 8 KiB (front matter looked at twice, never held; a tag is
    made in a 48-byte buffer, so a longer one is no tag before anything is allocated; the
    interface-6 `to_ascii_lowercase` of the key is gone).
  - `page`: the window of 100 rows. A source offers borrowed rows (`RowRef`), so an entry the
    window does not keep costs nothing; the window reserves its 101 rows once; order is compared,
    not keyed (no key strings), and "latest first" no longer negates the time (`i64::MIN` is no
    overflow).
  - `source`: a folder, the notes of the index, the notes with a tag; the path of each entry is
    made in one reused buffer.
  - `index`: the walk, the tagging, the state file, the tag counts, the notes of the index. The
    files are plain names inside the plugin's *data* folder (`index.txt`, `tags.txt`,
    `state.txt`); the vault is a second storage (the card). A walk that ends in `NoMemory` is over.
- `tests/native_alloc_refusal.rs` refuses each allocation of all of the above in turn (the
  storages' own allocations are out of the count); it found an infallible allocation in the tag
  reader during the work.
- Two known differences from interface 6, both about the Greek final sigma. Names are put in order
  with each character lower-cased on its own (`ui_kit::text::cmp_lowercase`), not with
  `str::to_lowercase`: `ς` and `σ` sort apart. The search folds `ς` to `σ` on both sides, so
  `ΟΔΟΣ`, `οδος` and `οδοσ` are one word (interface 6 missed `οδοσ`); every query it found, the
  native search finds (`the_search_finds_greek_titles_as_interface_6_does`).
- `tests/native_declarations.rs` compares the group, the setting and its extras with what the
  interface-6 plugin registered, field by field. The one deliberate difference is the folder
  chooser: interface 6 named the screen `core.folder-editor`, the native setting names the slot
  `core.choose-folder` that screen fills (a plugin taking the slot's place edits the setting too).

## Ported in slice 2 (`native_plugin.rs`, `native_home.rs`, `native_list.rs`, `native_tags.rs`, `native_paged.rs`, `native_card.rs`)

- Declarations (`declarations::ui`, part of the image): the screens `notes-vault.screen` (the page,
  one instance), `notes-vault.list` (many: a folder opens in a list of its own) and
  `notes-vault.tags`; the action `notes-vault.open`, its Tools entry (order 38, icon `note`), the
  group's icon, and the subscription to `event.work_due` (token 20). `tests/native_ui_declarations.rs`
  compares them with what interface 6 registered, and the host admits them.
- Metadata: services `core.files`, `core.ui`, `core.work`; feature `uses.storage` (the card root,
  read only).
- The page (`native_home.rs`): *Folders*, *Recent notes*, *Tags*, *Search notes*, *Update the index*
  with its status line (how far, or how many notes and how old); without a vault one row that asks for
  the folder. The index is made by the work service: `event.work_due` calls `Home::work`, which reads
  the card for 200 ms (`SLICE_US`, by the host's monotonic clock) or until the index is made, redraws
  the page, and declares the work again (the host runs it no sooner than 1 s later: `work.md` 1.1).
  The tags follow the index, as on interface 6. The page re-reads the vault's folder setting and
  the age of the index whenever it is shown (no `event.setting_changed` subscription: the host does
  not admit it yet, #763), so a folder chosen in the settings begins its index when the page comes
  back.
- The lists (`native_list.rs`): the window of at most 100 entries of `native_model::page`, *Earlier
  notes* / *More notes* rows where the list goes on, the note line (its folder in the vault), a note
  opens in the reader bound to `core.reader` (a hand-over with its `path`), a folder in a list of its
  own, a tag from the tags screen, the search from the keyboard question. A refused read of a later
  window leaves the window shown. The tags (`native_tags.rs`) are read from the file of tags.
- *Choose the vault folder* asks the host's folder question (`Question::folder`, the chosen folder
  written to the setting `notes-vault.folder`); the setting's own editor is still the slot
  `core.choose-folder`.
- The card (`native_card.rs`): `Storage` over `core.files` listings and the card root's reads, read
  only; a refused allocation is told as `NoMemory` whatever the model made of the failed read.
  It is a small copy of `file-manager`'s card adapter and moves into the kit with it.
- Differences from interface 6, on purpose: the index goes on while a list is open above the page
  (interface 6 paused it when the page was hidden; the host sends no hide event); the work's slices
  are 200 ms every second or so, not 5 ms steps (a screen has no timer on interface 7); a refused
  allocation or card read while indexing ends the work and the page says *The index could not be
  written* (retry with *Update the index*); the focus shows at once only where the last input was
  buttons.
- Gaps (ignored cases in `tests/acceptance_pending.rs`): nothing is drawn or taken through a host yet
  (H1/H4); the scripted host has no `core.files`, so the screens have no hosted test; a folder of
  thousands of entries is listed again from its start for each index step (the model asks
  `Storage::list_from`, whose default counts places: an X4 timing gate); a vault index of 50,000
  notes at about one step in each slice is slow, also to be measured on the device.
- Worst-case memory: one window of 100 rows (path, title) and the stack of folders being walked (at
  most 24 deep), the tags (400), the two 4 KiB `core.files` buffers while the card is read.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

- The **Markdown format** (`MarkdownDocument`, front matter, wiki links, tables): a format
  declaration (`sdk: format declaration through the typed codec`, #493) and the `core.format`
  provider with the document host (Host #417); the parser is not touched in this slice (it has
  about 60 infallible allocations to make fallible, then a provider as `format-txt`'s).
  `add_book_cache` (what is converted of each note) needs the book-cache declaration.
- The **home widgets** (recent notes, pinned note, note of the day, random note, tags, search): the
  widget host call (Core todo 76) and the ui-kit widget toolkit.
- The vault's folder editor is the slot `core.choose-folder`; interface 6 named the screen
  `core.folder-editor`. Same chooser; the registry-through-loader case checks it.
