# Interface-7 port (slices 1 to 3: the model, the declarations, the native entry and screens)

Version 1.0.8 (already above its Beta 3 version; no further bump). Status: **not complete**: the native
entry (`NativeFontsBrowser`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin) shows the list and a
font's page, but downloading, installing and removing a font are named blocked steps below, so the plugin is not
in `tools/plugin-files7/complete.txt`. The interface-6 `FontsBrowser` and everything of it stays until the host
activates native plugins (then it goes together, no shim; see the removal step).

## Ported

- `declarations.rs`: the screens (`font`, `list` single), the catalog setting
  (`fonts-browser.catalog`, developer group), the actions *More fonts* / *Your own fonts* with their
  places after the reading font's and the status font's choices and their icons, the events
  (download done, started). `tests/abi7_declarations.rs` compares each with the interface-6
  registration.
- `src/native_catalog.rs`: the catalog JSON (`Catalog::event` takes what the stream parser reports;
  `Catalog::read` runs the parser), font and file fields, the files this device takes (packs, or
  font files where scalable fonts are drawn), addresses resolved against the catalog's (`join_url`,
  pinned equal to the SDK's), the font as a screen parameter (`to_param` / `from_param`), versions
  (`newer`). The same rules and the same result as `catalog.rs` (a test reads the same catalogs with
  both); allocations are fallible and swept.
- `src/native_plan.rs`: which files of a font are downloaded where (the data folder's `staging/<plugin>/`,
  `core.net` `save_to` root 0, `max_file` 512 KiB for packs and 1 MiB for font files as in
  `docs/abi/network.md` §8) and checked (path inside the folder, SHA-1, size, `plugin.ini` among them),
  and the folder that `install_plugin` is told to install.

## Native entry and screens (slice 3)

- `src/native_plugin.rs`: `make` submits the screens, the catalog setting, the actions with their menu places and
  icons, and the `http_done` subscription. *More fonts* opens the list; the list shows the catalog kept in the data
  folder (`catalog.json`, read by `Catalog::read_file` in 2 KiB pieces, never whole: it may be 512 KiB) at once and
  asks `core.net` for a fresh one once each time it is opened (`catalog.json.part`, `max_file` 512 KiB, kept only if
  it reads as a catalog, the answer's final address resolving the files' addresses), with the interface-6 troubles
  (loading, no connection, not reachable; tapping the empty list tries again). A row is the name, what it is
  (category, styles, author) and where it stands (reading, update, installed, else its size). Choosing a font hands
  its page the font as the one screen parameter (`Font::to_param`) and opens it; the page shows the sentence, what it
  is, the licence, and the rows of `native_view::rows` (*Read with*, *Use for the status lines*: `set_setting`).
  Installed fonts are the plugins `font-*` of the registry (`core.registry`, op plugins), the device's font kind
  (files or packs) comes from `core.device` (memory class, `outlines`).
- `src/native_view.rs`: the rows of a page and the installed versions, tested without a host.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- **Download, update, remove (named blocked step):** *Download*/*Update*/*Stop*/*Remove* answer `Unsupported`.
  Needs `CommandSender::install_plugin`/`remove_plugin` (PR 621, waiting for its bless) and event 97
  `install_done` (#589). The plan is `native_plan` (files to the data folder's `staging/<plugin>/` with `core.net`
  `save_to`, each checked with its SHA-1 read in pieces, then `install_plugin` for the folder with the update flag).
  The host asks the user (decision 132) and its dialog is not wired yet (Kernel todo 145): the plugin must take
  `Unsupported`/`Denied` as an answer. The job state (which file is next) is kept in the data folder.
- **The restart's cancel:** interface 6 subscribed `started` to cancel a download cut off by a restart; the native
  entry has no download yet, so it does not subscribe: it comes back with the download.
- The *Download*/*Update*/*Stop*/*Remove* rows are not shown until then (no dead buttons); `native_view::rows` still
  says which rows a page will have.
- **Your own fonts:** the page of the help opens in the reader with `command.open` into `core.reader` (root 5
  "handed"; as About's licences): `Unsupported` until that merges.
- **The font's own face** in the list and on the page: needs a font loaded at run time (`FONT-runtime`).

## Old list of what was missing before slice 3 (kept for the record)


- The screens and the download job (H1, H4): the list fetches the catalog with `core.net` into the
  data folder (`uses.network`), the font page downloads file by file (up to 4 requests per plugin),
  verifies each SHA-1 with the SDK's digests reading the file in pieces, then installs.
- Installing: `command.install_plugin` (root 0, the staging folder, `uses.install`, flag replace for an
  update) has a wire form but no `CommandSender` method; the host asks the user (decision 132) and the
  plugin sees `install_done`. The job state (which file is next) is kept in the data folder as the
  interface-6 `Installer` did, so a restart begins the download anew from the font's page.
- Samples in the font's own face need a font loaded at run time; fonts are declared in metadata only
  (`resource.font_face`), so the native list shows the names in the interface font until the host has
  a contract for it.
- The catalog is read with `Parser::try_feed`/`try_finish` (bounded: 64 levels, 4,096-byte strings,
  256-byte keys); a refused allocation ends the read in `Error::NoMemory`. The refusal sweep covers
  the parser as well as the catalog.

## Review fixes

- Only `install_done` with status OK and reason 0 says *ready* (`native_plan::ended`); a refused,
  dismissed or timed-out confirmation and any failure say nothing of the kind, and the
  `staging/<plugin>` folder is removed in every case.
- A font the plan cannot install (a file over 512 KiB for packs or 1 MiB for font files, a size of
  0, a path that leaves the folder, a plugin id over 40 bytes) is not offered
  (`native_plan::fits`, used by `Catalog::finish`).

## Removal step (one sweep, with the reader port and Core #944)

1. `FontsBrowser` and its `Plugin` impl, `list.rs`, `font.rs`, `install.rs`, `samples.rs`, `catalog.rs` (the interface-6
   catalog; `native_catalog` is the same rules), the `HOW` table kept for the manifest;
2. the app's store-offer line and the other plugins' tests that register `FontsBrowser`;
3. list `NativeFontsBrowser` in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt` once download, update and
   removal are native.
