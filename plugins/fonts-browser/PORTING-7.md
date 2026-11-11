# Interface-7 port (slices 1 to 4: the model, the declarations, the native entry and screens, install and removal)

Version 1.0.8 (already above its Beta 3 version; no further bump). Status: the interface-6 `FontsBrowser`
is **removed** (this port: the plugin, its list and font screens, the installer, the samples, `catalog.rs`, the
plugin-file build of `tools/plugin-files`, the app's store-offer line, the store tests' use of it). The native
entry (`NativeFontsBrowser`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin) is what is left. **The
current interface-6 app loses the fonts browser until native activation (#243): merge right after it.** It is
not yet in `tools/plugin-files7/complete.txt`: list it when download, update and removal run end to end (H2).

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
- Gaps: the host's questions are not wired to native
  plugins (H2), so no end-to-end run yet (`fonts-browser-install` stays ignored).

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

## Interface-6 suites replaced

| Removed interface-6 test | Native coverage |
|---|---|
| `catalog`: `fonts_and_files_past_the_limits_leave_those_kept_whole` | `native_catalog/tests.rs` `fonts_over_the_caps_are_not_offered`, `the_catalog_reads_as_the_interface_6_reader_did` (what the old reader made, pinned as literals) |
| `catalog`: `only_the_files_this_device_takes_are_kept` | the same two (packs or outlines per device) |
| `catalog`: `a_font_goes_to_its_page_whole` | `native_catalog/tests.rs` (same name) |
| `samples`: `samples_are_held_within_their_budget` | ignored case `fonts-browser-samples` (`FONT-runtime`) |
| `install`: `the_catalog_is_shown_with_what_fits` | `native_catalog`, `native_view` tests; screens: ignored `fonts-browser-screens` |
| `install`: `a_font_is_downloaded_chosen_and_removed` | `native_job/tests.rs`, `native_plan/tests.rs`; end to end: ignored `fonts-browser-install` (H2) |
| `install`: `a_damaged_download_is_not_installed` | `native_job/tests.rs` `a_file_is_kept_only_if_size_and_digest_are_the_catalogs` |
| `install`: `updating_the_font_one_reads_with_keeps_it` | ignored `fonts-browser-update-keeps-font` (H2) |
| `install`: `coming_back_to_the_list_fetches_nothing` | `native_view::should_fetch` test; ignored `fonts-browser-list-back` |
| `install`: `a_font_page_has_its_font_from_the_list` | `native_catalog` `a_font_goes_to_its_page_whole`; ignored `fonts-browser-list-back` |
| `install`: `a_download_cut_off_by_a_restart_is_dropped` | `make` clears `staging`; ignored `fonts-browser-restart-drops-download` |
| `install`: `your_own_fonts_shows_how_to_read_with_a_font_file` | ignored `fonts-browser-own-fonts` (`draft-22-doc_open-root-5`): the native action answers `Unsupported` meanwhile |
| declaration tests (compared with the interface-6 registration) | `tests/abi7_declarations.rs` pins the decoded records as literals |

## Set aside until native activation (todo 380)

- Six tests of the store (`plugins/store/tests/store.rs`: `fonts_open_the_fonts_browser`, `discover_fonts_opens_the_list_where_the_fonts_browser_runs`,
  `discover_fonts_asks_for_the_restart_a_fonts_browser_waits_for`, `discover_fonts_offers_to_switch_a_switched_off_fonts_browser_on`,
  `larger_sizes_are_discovered_from_the_reading_settings`, `the_list_shows_what_fits_by_group`) are `#[ignore]`d: they need the browser registered.
- *Your own fonts* does nothing natively until the reader can be opened with a handed page (above).

## Notes for activation (Port Reviewer, #1058)

- The native plugin has no `HOW`/`CATALOGS` statics, so `how/*.txt` is unused until the rule for plugin help texts is picked (as #996 to #999).
- The store tests' `discovering(fonts, ...)` ignores its argument now: drop the parameter when the six tests return.
- Put `fonts-browser` back into the app's `NETWORK_SETTINGS` (memory only).

