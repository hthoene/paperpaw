# Catalogs (OPDS) on interface 7

Status: **native only; complete in code, not accepted on a host.** The interface-6 plugin (list, editor,
browse, downloads, covers, sync, readers) and its kernel-driving tests are removed. `NativeOpds`
(`NATIVE_ENTRY`, features `linked` and `native-entry`, `native/metadata*`, `build.rs`) is the plugin; it is
linked into the app's native list (the app depends on the crate only with its feature `native-linked`, as for
the other native ports; the default app no longer installs Catalogs and `opds` left `NETWORK_SETTINGS`), and
`opds` is in `tools/plugin-files7/complete.txt`. Host acceptance waits
for the activation of native plugins (Core #944, Host #417) and the shells' production `core.net`, work,
library and image wiring (`tests/acceptance_pending.rs`). X4 file size and peak memory are not measured.

Version 1.0.4: Beta 3 was 1.0.3 and the first slice bumped it; no further bump before 1.0.

## What it does

- Declarations (`declarations.rs`, `build.rs`, `native/metadata*` by `tools/abi-metadata.py`): the two settings
  (covers in lists, books per sync) in the catalogs' part of the library group, the screens `catalogs` (one at
  a time), `browse` and `edit` (as many as are opened), the actions `open` and `sync`, the Tools entry
  (order 45), the icons, and the subscriptions to the end and the progress of a request, work coming due, a
  picture ready and the plugin stopping. Services: `core.net`, `core.library`, `core.secrets`, `core.ui`,
  `core.device`, `core.document`, `core.work`; features `uses.network`, `uses.library`. En/de texts and the
  `how/` text are unchanged but for the new `opds.not-kept`; `how/` stays in the crate for the store's page
  (there is no native place to register it).
- The list and the editor (`native_plugin/list.rs`, `edit.rs`, `native_catalogs.rs`): the list file
  `catalogs.txt` is read and written as version 1 had it (tab fields; the test reads a Beta 3 list and writes
  it back byte for byte). A list that cannot be read (too large, not text, more catalogs than the class holds:
  100 on a small device, 500 elsewhere) is an error that is shown and never saved over. The password of a
  catalog is a secret (`core.secrets`, named by 16 letters of the digest of the address, changeable by the
  user, wiped by `mem::wipe` wherever a buffer held it, `Debug` shows nothing); a password an older version
  kept in the list moves to the secrets on the first open, and the list lets go of it only once a secret holds
  it. Where the device keeps no secrets the shared text `core.secrets-unsupported` says so (an older
  password in the list keeps working). A new address takes the password along; a removed catalog's secret
  goes. An empty answer for the password removes it (as in interface 6). The plugin may keep 32 secrets: past
  that a password is refused with `opds.not-kept`. Every change is kept at once; the folder, the name, the
  user are plain texts.
- Reading a catalog (`native_model.rs` OPDS 1, `native_json.rs` OPDS 2, `native_dav.rs` WebDAV listings,
  `native_search.rs` search descriptions and addresses): all fallible (the XML decoder's `try_decode` and
  `Attrs::try_get` closed GATE-ENTITY; the JSON parser's `try_feed`/`try_finish`), read in pieces from a file of
  the data folder, a refused allocation ends the reading with what was read, the feed marked `memory`. Limits:
  a link over 1,024 bytes or a type over 100 is not kept; a page holds at most 100 entries and 64 KiB of text
  on a device with room, 40 and 24 KiB on a small one (the X4); the sync's queue of books at most 96 KiB / 24
  KiB of text; the trail of feeds 32 / 12.
- Browsing (`native_browse.rs`, `native_plugin/browse.rs`): the engine takes the events of the requests and
  returns what the screen is to do; one request of each kind at a time (a feed, a description, a book, a cover,
  and the sync's two), each answer in a file of the data folder (`feed.tmp`, `description.tmp`, `book.tmp`,
  `cover.tmp`, `sync-feed.tmp`, `sync-book.tmp`) that goes once it is handled, at a stop and at the first open;
  an answer for another request is ignored; links are resolved where the answer came from after redirects
  (`Net::answer`); the catalog's password goes only to the catalog's own address (scheme, host, port), never to
  a link elsewhere (the password goes with Basic as the catalog is entered, so a catalog at `http://` sends it without TLS, as in Beta 3). Back stops a download, else goes back through the feeds, else leaves. Search by
  template or by description (asks where to search first). A line below says how a load or a download goes (its
  share every 10 % or 5 s, or its size).
- Downloads (`native_download.rs`): the book goes to `book.tmp`, is checked by its first bytes (a page to sign
  in is no book), and put into the catalog's folder below the library folder with `Library::import` under a
  free name (`FreeName`), so no book of the reader's is replaced; stopped or failed leaves nothing. The name
  is the first author and the title, cleaned (never a path, 120 UTF-16 units), else the name in the address.
  Saved, a choice offers to read it: the book opens in the reader the user bound to `core.reader`.
- Covers (`native_covers.rs`, drawing in `native_plugin/browse.rs`): on a device that is not a small one and
  with the setting on, the covers of the entries on the page shown load one after the other (one download at
  a time, at most 6 images held of the host's 8, released when the feed or the screen is left), are kept in a
  cache of files (`covers/<md5>.img`, the 48 newest) and decoded by the host (`PrepareImage`, side 160). A
  cover of a feed shown before is never shown for another entry.
- The sync (`native_sync.rs`, `native_plugin/sync.rs`): the interface-6 rules as an engine: the catalogs marked
  for it one after the other, pages of the list read in turn (at most 50, never twice, tracked by digests), the
  books not in the record downloaded one at a time into the catalog's folder and put in with `Existing::Refuse`,
  at most the setting's limit a catalog; the record `synced/<md5 of the address>.txt` (one id a line, an id over
  512 bytes by its digest, read line by line, never taken for empty when unreadable: the catalog is then
  skipped); a catalog that cannot be reached, a list of lists only and a skipped one are counted and reported;
  a refused allocation ends the run. The list's *Sync catalogs* row and a catalog's *Sync now* run it at once
  (the row stops one going); the action `opds.sync` syncs at once when the reader runs it and, run in the background (a rule, another
  plugin; `event.action_run` flag `background`, #885), declares work (`when_cheap`, within the hour) that
  `work_due` runs, as interface 6 did.

## Differences from interface 6

- No offer to go online first (Offline mode): a request ended for the power mode or the network says
  `opds.no-network`.
- The folder is below the library folder (`Library::import` takes no other place). One written from the
  card's root as interface 6 had it (`Books/Calibre`) is read as below the library, leaving out a leading
  library folder name. The folder chooser is not used: a text question.
- A book of a sync whose name is taken in the folder is fetched once, refused by the library and remembered
  (interface 6 looked at the card first and did not fetch it).
- The progress of a download is a line of text, without the bar below it.
- Covers: the kit's rows have no place for a picture, so a row's tap-zone column keeps the room and the cover
  is drawn over it (half a row wide; interface 6 had two thirds). A cover is tried once for a feed.
- Leftovers of Beta 3 are not removed: `tmp/<n>.<ext>` and `covers/<md5>.gray` (the data folder cannot be
  listed from a plugin). The new files have fixed names and are removed at the first open and at a stop.

## Not done, and what it waits for

- **`Library::provide` (LIB op 24, Services' draft PR #747):** a downloaded book's title and first author, which
  interface 6 gave the library, are not handed over. Nothing is kept for a later replay: when `provide` exists,
  the browse download (`Browser::downloaded`) and the sync's store step (`Sync::stored`) are the two places to
  call it with `entry.title` and the first of `entry.authors` for the path `import` returned.
- **Host acceptance:** the plugin through the production loader needs the activation (#944, #417) and the
  shells' `core.net`, `core.work`, library import and image wiring. No scripted test drives `native_plugin.rs`
  itself (the screens, the questions, the drawing): the engines are driven through scripted `core.net` and
  `core.library` services (`tests/native_browse_flow.rs`, `tests/native_sync_flow.rs`), the readers, the list,
  the passwords and the covers' state without a host; `tests/acceptance_pending.rs` keeps ignored cases that
  name the missing piece.
- **A picture place in the kit's rows** (the covers' column is a stand-in): it would let the plugin do what
  interface 6 did.
- **Not measured:** the X4 file size (`tools/plugin-files7`) and the peak memory of a browse page, a cover
  page and a sync on the X4 heap. On the checklist: a page of at most 40 entries and 24 KiB, the sync queue
  of 24 KiB, 6 images, one 4 KiB frame buffer for each request, the cover cache of 48 files.

## Tests

`tests/native_catalogs.rs` (list, passwords, folders), `native_feeds.rs` (the readers on the servers' feeds, hostile
ones, class limits, properties), `native_covers.rs`, `native_browse_flow.rs` and `native_sync_flow.rs` (against
scripted services: requests as sent, headers, files, imports, what each answer leads to, every refused call and
allocation), `native_alloc_refusal.rs` (every allocation of the host-free code), `native_declarations.rs`,
`native_translations.rs`, `native_linked_entry.rs`. `tests/fakes` holds the data folder and the secrets in
memory, and the scripted services.
