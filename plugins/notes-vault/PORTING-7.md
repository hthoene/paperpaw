# Notes vault on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (the Markdown format, the vault's page, lists and
tags screens, the index, the home widgets, the registration) is still in place beside the native
one. It is removed in the PR that finishes the port.

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
- One known difference from interface 6: names are put in order, and searched, with each character
  lower-cased on its own (`ui_kit::text::cmp_lowercase`), not with `str::to_lowercase`; they differ
  for the Greek final sigma only.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

- The **screens** (the vault's page, the lists, the tags): `core.ui` compose (Input H3), the
  keyboard question for the search, and the **card adapter** over `core.files` for the vault
  (`file-manager`'s `card.rs` is one; it belongs in the SDK or in `paperpaw-ui-kit` before a second
  plugin copies it). The index runs "a piece at a time while the page is open" on interface 6; on
  interface 7 that is the work service (`work.md`, no SDK wrapper yet) or the page's own ticks.
- The **Markdown format** (`MarkdownDocument`, front matter, wiki links, tables): a format
  declaration (`sdk: format declaration through the typed codec`, #493) and the `core.format`
  provider with the document host (Host #417); the parser is not touched in this slice (it has
  about 60 infallible allocations to make fallible, then a provider as `format-txt`'s).
  `add_book_cache` (what is converted of each note) needs the book-cache declaration.
- The **home widgets** (recent notes, pinned note, note of the day, random note, tags, search): the
  widget host call (Core todo 76) and the ui-kit widget toolkit.
- The **tools entry** (order 38, icon `note`) and the action: they are declared with the page, so
  that no entry opens nothing.
- The vault's folder editor is the slot `core.choose-folder`; interface 6 named the screen
  `core.folder-editor`. Same chooser; the registry-through-loader case checks it.
