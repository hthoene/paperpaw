# Files (the library) on interface 7

Status: **native only.** The interface-6 plugin (`Files`, its list, shelves, names, information and
folder chooser screens) and its kernel tests are removed; the native plugin is linked
(`NATIVE_PLUGINS`) and listed in `tools/plugin-files7/complete.txt`. Host acceptance of the native
screens waits for the activation of native plugins (#944, #417; `tests/acceptance_pending.rs`). The
X4 file builds and packs (`tools/plugin-files7/build.sh x4 <out> --only files`: format 5, 147,692 B
of code, 23,888 B of data, 174,116 B in all, no frame of 2,048 B or more); its size against the
Beta 3 file (the flash gate) and its peak memory on a device, with covers, are **not measured** yet.
Version 1.0.7 is above Beta 3 (1.0.6): no bump.

## What is here

- Declarations (`declarations.rs`, built into the image by `build.rs`; `native/metadata*` generated
  with `tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
  native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`): five
  settings, the screens (the library, a list of books of one scope, the shelves, a list of names, book
  information, the folder chooser), the actions of the shelves and the search, the library menu
  (`files.menu`: order, view, search, shelves, the library folder, settings), the slots `core.library`,
  `core.home` (fallback), `core.book-info` and `core.choose-folder`, the subscription to
  `library_changed`, and five help hints. The plugin uses `core.library`, `core.ui`, `core.files` and
  `core.device`, and the signed feature `uses.storage` (optional: delete and the archive's move need it).
- Model, free of the host: `native_model` (filters, order, scopes, the window of at most 96 books),
  `native_cells` (the cells of the list and the words of every state), `native_states` (preparing,
  scanning, folder unreadable, card gone), `native_list` (cells, pages of a list or a grid, what a
  button or a tap means), `native_covers` (which book has its cover, which waits, which has none),
  `native_sheet` (what holding a book offers, the archive's free name), `native_shelves`,
  `native_info`, `native_choose`. All allocation is fallible; `tests/native_alloc_refusal.rs`
  refuses each allocation in turn.
- The host side: `native_plugin.rs` and `native_plugin/` (books, names and shelves, opening, the
  sheet, the chooser, the sources of replies), drawing in `native_draw.rs`.

### Covers

`Library::cover` (LIB op 7, side 0: the list's size) is asked while a page is drawn, for the books
on it, and `Canvas::draw_image` draws the image id (list rows, the grid's tiles, the card of the book
read last, the book information). A book that has none, or a cover that cannot be had, is an outline
with the title's first letter. `Busy` (being made) is asked again when `library_changed` bit 6 says
one is ready; an id the host ended (`NotFound` when drawing) is asked for once more. At most 6 ids are
held (3 on a device of the small class, from `core.device` `.class()`, as opds does) and an id is given
back (`image_release`) as the page changes and when the screen closes. The covers are not skipped on
the small class: the plugin holds no picture, only ids; whether the X4 makes covers fast enough is
unmeasured.

### The list

The library opens with the filter row (steps all, reading, unread, finished, shelves, with the
count of the books), the book read last as a **card** (the library in the order of reading, its first
book begun and not finished), then the books: cover, title, how far it is read (or *Finished*, *New*),
author and series, the badges as **chips** and a **bar** for the progress. The grid view
(`files.view`) shows tiles of covers, 2 to 4 across by the display's width. Long libraries slide in
windows of 96 books, "More books" and "Previous books" (`native_model::read_window`).

### States

`Library::state`: *Empty* is "Preparing the library …"; *Walking* leads with **"N books so far …"**
(`books`, English and German, the books found are listed under it, with `walk_ppm` as a rough bar
(whole per cents from 1 to 99, nothing under 1: a first walk has no total to measure by)); a refused ask
is "Couldn't read the books folder", or, where `core.files` `stat` of the card
root says `ERR_IO`, "Restart PaperPaw" (card gone); a library with books only in the archive says so
(`files.list.none.all`). Each opens its list, never fails the screen.

### The book sheet (hold a book, or the menu button)

A choice dialog (`Ui::ask`): open, *Mark as finished* / *Mark as unread* (`Library::set_finished`),
*Book information*, *Move to the archive* (offered where the archive folder is set and the book is not
in it) and *Delete …* (offered where `Library::can_delete` answers OK). The move is `core.files`
`move_entry` into the archive folder, never replacing: a taken name is tried as `name (2)` and on, up
to 9; the host follows the move itself. Delete asks first and starts on the safe answer, then
`Library::delete` with notes (the question says so); a refusal is told in a notice.

### The rest of this slice

- **Library menu**: order and view are the settings as menu items, then search, shelves, the library
  folder (`core.library-folder`) and the settings (`core.settings`).
- **Shelves** list the favourites and the authors, series, tags and folders with their counts at the
  row's end; the lists of names show the count at the end too.
- **How it works** is `files.plugin.how` in the catalogs (English and German; `how/` stays in the crate
  for the store's page); **help hints** are declared for the library screen.
- **Live catalog**: the lists read again on `library_changed` (books, status, fields, the walk).
- **Find by letter**: the last row of the shelves, *Find a book* (`files.list.show.find`, with the
  count of letters), opens the names screen of `Shelf::Letters` (the `letters` facet asked as the
  library's own list is: all books, the archive left out): one row for each letter, *Books under B*,
  with its book count. A letter opens the library by title (`Order::Name`, filter All, the order
  `fixed` so a change of the setting does not replace it) at the window that holds the letter's `first`
  book, the focus on that book. A list cannot start at an index (a cursor is only made by a reply), so
  `native_model::find_window` reads windows of at most 96 books from the start cursor as "More books"
  does, counting the books, and keeps each window's start for "Previous books"; at most 96 windows
  (9,216 books or more) are read, a stale cursor starts the walk over once (a second is `State`), an index
  past the end gives the last window and its last book. The letter's `first` is read again when the
  letter is chosen (not kept from the names screen, so a changed catalog cannot make it stale); a letter
  that is gone, a `first` of `0xFFFF_FFFF` or a refusal opens the list from its start. The window is read
  once to find the book and once more to show it. Each host call of the walk skips `cursor` matches in the
  host (its checkpoints make that cheap, `library.md`); the cost on a device of a letter far down a
  library of thousands of books is **not measured**. Untested without a host: that the focus really lands
  on the book's row (the scripted host has no screens; `Browser::focus_book` is not observed), the
  drawing of the letters screen, and that the host's `first` agrees with the title order of the list
  (`library.md` says it does; the host's catalog is not qualified). Nothing else is needed from the SDK
  or the ABI.
- **Folder chooser** (`files.choose-folder`, the slot `core.choose-folder`, parameter `id`): starts
  on the folder the setting has (the top where it is gone), the first row uses the shown folder, then
  the way up and *New folder*, then the folders (at most 256, by name, hidden ones left out), read a
  page at a time from `core.files`; Back goes up a folder before it leaves.
- Tests: unit tests of each model module, `tests/scripted_browser.rs` (what the list asks the
  scripted library, the states, library changes, the sheet's choices, the archive's move, delete asked
  about first), `tests/scripted_choose.rs`, `tests/scripted_library.rs` (also what the
  letters screen and a letter's list ask a scripted library of 100 books, and refusals), `tests/native_*`, the refusal
  sweeps. The scripted host has no screens: a redraw, a notice and an open are refused there after the
  plugin asked the services, so those tests look at what was asked and submitted. Drawing is not tested
  without a host (`acceptance_pending.rs`).

## Differences from interface 6

- The shelves are one list of categories, not names inline; the folders list is one level (the
  folders right inside the library folder that hold a book at any depth).
- *Next in your series* lists series being read first, then finished ones, each in the host's
  alphabetical order, at most 48 series.
- Search has no 500-books cap (the host has none).
- The filter stays one row that steps through the filters; there are no chips for it on large
  screens (the kit has no chip widget).
- The library is a list of windows of 96 books; interface 6 showed one list of the whole index.
- The setting `files.covers` (black and white or gray) has no effect: `Library::cover` and
  `ImageStyle` have no such flag, the host's drawer chooses.
- The folder chooser serves the settings' editor only (`id`): the `reply` event and a dialog asked with
  `Question::folder` (no asker exists in the tree) are not served.

## Still missing (each returns with its piece)

- **Favourites cannot be added or removed.** The facade `Library` has `set_finished` but no
  `set_favourite` (`lib_status`, bit 1 favourite, in LIB op 8): the sheet has no row for it. The
  favourites list and its shelf work for books marked elsewhere.
- **Look for new books** (a request for a new catalog): no typed command.
- **The search gesture on the home screen** (`swipe.down`, a keymap fragment): there is no declaration
  of one in the native contract.
- **Book information** lacks the tags' text (LIB op 4 has no facade) and the language and the
  description (the format's, `native::document` metadata).
- **Gray covers** apart from black and white (see above).
- **Help hints** are declared but the host's `validate_record` does not admit them yet
  (`ERR_UNSUPPORTED`); the plugin submits them best effort and loads without them. The menu entries
  wait for the host's admission like every plugin of this port.

## Pending host acceptance (each is an ignored case in `tests/acceptance_pending.rs`)

- The native screens' drawing, composition and input (H1/H3/H4) and the host's catalog behind LIB
  ops 2, 5 and 6; the covers on the device (image slots), the sheet, the chooser, the menus and hints
  (#944, #417).
- The size of the plugin file on the X4 and its peak memory with covers: not measured.
