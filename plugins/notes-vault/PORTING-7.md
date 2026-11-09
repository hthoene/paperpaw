# Notes vault on interface 7 (slices 1 to 3, partly ported)

Status: **not complete.** The interface-6 plugin (the Markdown format, the vault's page, lists and
tags screens, the index, the home widgets, the registration) is still in place beside the native
one; the screens are native since slice 2, the Markdown format since slice 3 (up to what the host takes), the widgets are not. It is removed in the PR that finishes the port.

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
- A refused first read of the card (opening a screen) leaves a one-line screen (`native_notice.rs`)
  that says why and goes back, not a dead screen: "allow card access" where `uses.storage` is denied,
  "the card could not be read" for any other refusal; a refused allocation is the host's. The page
  says the same where the work finds the card denied. No hosted test yet (the scripted host has no
  `core.files`): the notice's mapping is unit-tested.
- X4 checklist (not measured): the two 4 KiB `core.files` buffers of `Card`, and the index timing.
- Worst-case memory: one window of 100 rows (path, title) and the stack of folders being walked (at
  most 24 deep), the tags (400), the two 4 KiB `core.files` buffers while the card is read.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

- The **home widgets** (recent notes, pinned note, note of the day, random note, tags, search): the
  widget host call (Core todo 76) and the ui-kit widget toolkit.
- The vault's folder editor is the slot `core.choose-folder`; interface 6 named the screen
  `core.folder-editor`. Same chooser; the registry-through-loader case checks it.

## Ported in slice 3: the Markdown format (`src/markdown*`, `src/fallible.rs`, `src/native_format.rs`)

- The parser is the interface-6 one, made fallible: every growth on the way of opening and reading a
  note (`markdown.rs`, `front`, `table`, `wiki`) goes through `src/fallible.rs` (a small copy of
  format-epub's: that crate's is private) or `try_reserve`; a refusal is `DocumentError::OutOfMemory`,
  never an abort. The vault walk (pictures an embed names, below the `.obsidian` folder) keeps what the
  listing's callback cannot answer in a `Walk` and ends the open. `tests/markdown_allocation.rs`
  refuses each allocation of opening and reading a vault note in turn (front matter, wikilinks,
  embeds, a table, a callout, a code block) and checks the output is a clean run's, and that a retry
  reads. The same code serves interface 6 (the registration stays until the host activates native
  plugins).
- Differences, on purpose: file names of the picture walk and of the callout kind are lower-cased
  character by character (as the names of the index are, slice 1), so a Greek name that ends in a
  capital sigma differs from `str::to_lowercase`; a picture that is on the card and cannot be decoded
  is the format plugin's (`image_error` words the decoder's error without asking for room): the sweep
  leaves such a file out.
- `native_format::markdown_provider()` is the shared `FormatProvider` of format-epub over
  `MarkdownDocument`; the call's storage is `VaultIo`: the note is root 3 (`path ""`), the
  conversion root 6, and every other path is the card (root 1, read only, `core.files` through
  `native_card::with_card_or_none`: a card the plugin may not read is one with nothing on it, the
  note then reads without a vault). The card path of the note is the call's `text` (Kernel: the host
  puts it there for a format that declares `uses.storage`, where the book came from the card; todo
  336); with none the note is read without a vault, and its title is empty (root 3 has no name: the
  library names an untitled book by its file, as for format-txt). `tests/provider.rs` compares the
  provider with the interface-6 reader on the same vault note block by block (texts, runs, maps, spans,
  metadata), with and without a card path, and checks `VaultIo`'s routing and refusals and hostile
  paths.
- `declarations::formats` declares `notes-vault.markdown` for `md` and `markdown`, priority 10 (the
  format plugins' own Markdown is priority 0; the user's choice of format for an extension wins over
  both). `make` submits it after the settings and the screens and goes on where the host answers
  `ERR_UNSUPPORTED` (it takes no `format.spec` yet, `declarations.rs` admits it and answers
  unsupported). The `core.format` calls arrive as `Event::StageCall` (`stage "core.format"`), need
  no screen, and are answered by `native_plugin::format_call` (reply of at most 3,840 bytes); the
  provider keeps its documents between calls.
- Gaps (ignored cases): the host takes the declaration (`BLOCKED DOC-format-declaration`, Host #417 /
  Core #944), puts the card path in `fmt.call.text` (`BLOCKED DOC-format-card-path`, Kernel todo 336,
  Host todos 329/109); so a note cannot be opened through the real document service yet.
  Activation list (cases to un-ignore when the host lands, in `tests/acceptance_pending.rs`):
  `BLOCKED DOC-format-declaration` (the host takes `notes-vault.markdown`) and
  `BLOCKED DOC-format-card-path` (the card path in `fmt.call.text`); both must pass before the
  interface-6 `MarkdownDocument` registration goes with todo 90. The
  `add_book_cache` registration has no interface-7 form: the host owns each document's cache (root 6).
  Each `core.format` call opens `core.files` and gives it back (two buffers of about 4 KiB); not
  measured on the X4. No scripted-host test of `Event::StageCall` (the scripted host has none).
  The shared provider's own allocation sweep is format-epub's (ignored there until Core todo 208).
