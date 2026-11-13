# Interface-7 port (slices 1 to 4: the model, the declarations, the native entry and screens, install and removal)

Version 1.0.8 (already above its Beta 3 version; no further bump). Status: **not complete**: the native
entry (`NativeFontsBrowser`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin) shows the list and a
font's page, downloads, installs, updates and removes fonts through the host (slice 4); under the app's
`native-linked-store` its settings come from the program's tables, and the flip branch lists it as a linked native
plugin for the activation merge (#2769, as the store's other plugins); it is not in `tools/plugin-files7/complete.txt` while
its interface-6 part stays. The interface-6 `FontsBrowser` and everything of it stays until the host
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

## Slice 4: download, update, remove (`src/native_install.rs`, `src/native_job.rs`)

- The page of a font offers *Download*, *Update*, *Stop* and *Remove* again (a font being
  downloaded shows only *Stop*; another font waits, `fonts-browser.busy`).
- `native_job::Job` (host-free): the plan's files one at a time; each file saved by `core.net` into
  `staging/<plugin>/…` (`max_file` as `native_plan` says) is checked by size and SHA-1 read in
  4 KiB pieces (`verify`); a file that does not check out, or does not arrive, ends the job and the
  staged files go (`clear`). Tested in `native_job/tests.rs`.
- `native_install.rs` (host): once the last file is kept, `command.install_plugin(1, Data,
  staging/<plugin>, update)`; the host asks the user (decision 132). `install_done` (new
  subscription, `declarations::installs`, token 4) ends it: only an OK outcome says *ready*, any
  other end says *could not be downloaded*; the staging folder goes in every case. *Remove* is
  `command.remove_plugin(2, id)` (the host asks, decision 124); its `install_done` refreshes the page.
- Metadata: the feature `uses.install` is added (the `.s3` files follow #713).
- Install requests: each download has its own request number (16 up); `install_done` ends a job only
  when it is that job's own, asked question, and no *Stop* is offered once the host was asked (the job
  stays until `install_done`), so a late answer cannot end the next font's job (test in `native_job`).
- Restart: `make` removes the `staging` folder (no job can exist then), which closes the cut-off-download
  gap that interface 6 closed at `started`; a download also clears its own staging first. Unmeasured:
  whether the host's `remove` takes a non-empty folder (an error is ignored, the leftover then goes with
  the next download of that font). *Remove* refused (Denied/Full) says it failed.
- Unmeasured X4 timing: checking a 1 MiB font file (SHA-1 in 4 KiB pieces) in one callback.
- The host's questions: `install_plugin` runs as a host job that asks the user before the swap (`plugin_install.rs`
  step 6, `plugin_confirm.rs`; declined is reason 2, dismissed 3), `remove_plugin` with its three-button question
  (decision 124); the plugin takes `Denied`/`Cancelled` as an answer. End-to-end on the flip emulator: #2769.

## Slice 5 (#2769): progress, follow changes, Offline mode, linked

- From #911 (not merged before): a download says how far it is (`2 / 5`, `Running::progress`) in the list and on its
  page; the pages follow a font chosen, installed or removed (`setting_changed`, `plugins_changed`); an empty list
  that says why tries again when confirmed or tapped; in Offline mode the list and a download offer once to go
  online (`uses.wifi`, `command.go_online`, `fonts-browser.offline-offer`); the button devices' wording.
- **Linked:** under `native-linked-store` the app takes its settings from the program (`fixed_tables`, the build
  script's tables). Listing `NATIVE_ENTRY` in `NATIVE_PLUGINS` (and offering the interface-6 `FontsBrowser` only
  without the feature) is on the flip branch `port-a/picture-home-linked-flip-228`, as for the store's other plugins.
- **Sizes:** a pack may be 4 MiB and a font file 8 MiB (`native_plan::MAX_PACK`, `MAX_OUTLINE`); interface 6 asked
  for no cap of its own, and the library's packs reach about 700 KB (512 KiB had kept 5 of its 6 fonts out).

## Your own fonts (`src/native_own.rs`)

- As Beta 3: the part of *How it works* after the heading `fonts-browser.own.heading`, under the title
  `fonts-browser.own`, is written to `own.txt` in the data folder and opened in the reader; where there is
  no such part or the card refuses the file, the notice `fonts-browser.how-failed`. The language is the one
  whose text has the heading shown (a language without a text of its own shows the English heading).
- The file is handed to the reader as Files Engineer's ruling says (#2376, #1186): `core.ui` op 16 with
  `root` `0` and `path` `own.txt`, then `command.open` into the slot `core.reader`.
- Memory: the part is appended straight from the text in the program (in 4 KiB pieces); only the title's
  line is allocated (a sweep pins one allocation). The `HOW` table is kept once for both entries.
- Tests: `native_own` (part per language, page layout, refused write, sweep), `tests/hosted_own.rs` (en
  and de through the kernel, nothing dropped).

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- **Download, update, remove through the production loader** (`fonts-browser-install`): done in the plugin (slice 4);
  the case stays ignored until the host qualifies H1/H2.
- **The font's own face:** on the font's page the sentence is drawn in the font's own face
  (`native_sample.rs`): the sample pack (one size, only the characters of the name and the sentence, asked
  from the catalog's `sample` address with `core.net` `save_to` into `samples/` of the data folder) is
  opened with `Font::acquire_data` (#1062) for the draw and released at once (a handle keeps up to 4 KiB of
  glyphs on the X4); a file that is gone or no font any more leaves the plain line, as does a library
  without samples or a device without a network. **Named differences:** the list's rows still show the
  names in the menus' face (a row has one text; one handle per row would take the 8 font handles); the
  sample is not held between screens (it is read from the card again).

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
3. list `NativeFontsBrowser` in `NATIVE_PLUGINS` (the flip branch, #2769, under `native-linked-store`); list it in
   `tools/plugin-files7/complete.txt` with step 1.
