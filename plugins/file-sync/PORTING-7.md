# File sync on interface 7

Status: **complete.** A new plugin made for interface 7 (draft 36, no interface change); there is no
interface-6 part. Version 1.0.0. A store plugin (`tools/plugin-files/plugins.txt`, format-5 file by
`tools/plugin-files7`), not linked into the firmware.

## What it does

Keeps folders of the card in step with WebDAV folders (Nextcloud, ownCloud, a NAS): up to three pairs
(settings `file-sync.folder-N`, `mode-N`, `server-N`, `user-N`, the password the secret `password-N`),
each two-way or download-only. The code is the UI kit's shared WebDAV module (`paperpaw_ui_kit::dav`:
addresses, the account header and the password among the secrets, the `PROPFIND` listing read as it
streams from a file) that Picture frame uses too.

## The rules (`native_plan.rs`, nothing else decides)

State per file: the card's size and time, the server's size and `ETag` (a hash; the size alone where
a server tells none), at the last sync. Changed on one side only: copied. Changed on both, or never
synced and another size: **both kept**, the card's copy renamed `name (conflict device date).ext` and
sent too. Gone on one side and unchanged on the other: gone on the other too, the card's copy going to
the trash (`trash/` of the data folder, the last 100, `trash.txt`); gone and changed: kept. Never synced
and the same size: taken as the same. Without a state nothing is ever deleted. In a folder where at
least five deletions would be more than half of what the state knows, they are held back (the files
are kept and go where they are missing). Download only never changes the server and mirrors it on the card.

## Safety

- A fetched file is saved by the host under `.pp-staged` and renamed: whole or not at all, the old one
  stays on any failure; a sent file is a `PUT` of a file read whole (`send_file`); streams, no file is
  held here. Stray `.pp-staged` files, hidden names (a dot) and `~` names are never synced.
- The state of a folder is kept (written under `.new`, renamed) after every file, so a cut or a lost
  network is picked up where it stopped; a state lost or odd only makes the next sync ask more (a
  conflict kept twice), never delete. A cut at every request, with the sent file arriving or not, is a
  test (`native_sync_tests.rs`).
- A folder with more than 256 files, deeper than six levels, a file over 64 MB (the host's limit for a
  saved answer): left as it is, said. A top folder missing on the server that was synced before is an
  error, not an empty server.

## Where it is not yet complete

- The library is not told of changed books: the host's catalogue walk finds them at its next look (no
  SDK call asks for one).
- Folders are not deleted or made empty on either side; renames count as a delete and a new file.

## Checks

Unit tests with a fake card and a fake WebDAV server (`native_sync_tests.rs`), declarations and
translations tests, the emulator run `tools/pps/file-sync.pps` (`tools/pps/file-sync/run-emulator.sh`)
against `testdata/fake_webdav.py`.
