# Interface-7 port (native only; the blocked cases wait for host pieces)

Version 1.0.8 (already above its Beta 3 version; no further bump). Status: **native only; complete in code, not
accepted on a host.** The interface-6 `FontsBrowser` (its list, font page, installer, samples and catalog reader,
and the tests that drove them through the kernel) is gone, with no shim. What its tests pinned that is pure is
pinned in the native tests (`src/native_catalog/tests.rs`, `tests/abi7_declarations.rs`); the end-to-end cases are
in `tests/acceptance_pending.rs`, blocked on the host. The plugin is `NativeFontsBrowser` (`NATIVE_ENTRY` with
the feature `linked`), listed in `paperpaw_app::NATIVE_PLUGINS` (feature `native-linked`) and in
`tools/plugin-files7/complete.txt`; it is no longer in `STORE_PLUGINS`, and its interface-6 file is no longer built
by `tools/plugin-files`. X4 file: 167,168 bytes (`tools/plugin-files7/build.sh x4 … --only fonts-browser`), no frame
of 2,048 B or more.

## What it does

- `declarations.rs`: the screens (`font`, `list` single), the catalog setting (`fonts-browser.catalog`, developer
  group), the actions *More fonts* / *Your own fonts* with their places after the reading font's and the status
  font's choices and their icons, and the events (`http_done` for the catalog and the files, `install_done` for the
  host's end of an install or a removal, and `setting_changed` / `plugins_changed` so that the pages follow a font
  that was chosen, installed or removed). `tests/abi7_declarations.rs` pins each as interface 6 registered it.
- `src/native_catalog.rs`: the catalog JSON read as it streams (bounded parser, fallible allocation, swept by the
  refusal tests), font and file fields, the files this device takes (packs, or font files where scalable fonts are
  drawn), addresses resolved against the catalog's (`join_url`, pinned equal to the SDK's), the font as a screen
  parameter (`to_param` / `from_param`), versions (`newer`).
- `src/native_plan.rs`: which files of a font are downloaded where (`staging/<plugin>/` of the data folder, `core.net`
  `save_to` root 0, `max_file` 512 KiB for packs and 1 MiB for font files as `docs/abi/network.md` §8 says) and
  checked (path inside the folder, SHA-1, size, `plugin.ini` among them), and the folder `install_plugin` is told to
  install. A font the plan cannot install is not offered (`native_plan::fits`).
- `src/native_job.rs`: the job as data without the host: the next file, how far it is, checking a file's size and
  SHA-1 in 4 KiB pieces (never whole), clearing what was staged.
- `src/native_plugin.rs`, `src/native_install.rs`, `src/native_view.rs`: the screens on `core.ui` item lists.
  - The list shows the catalog kept in the data folder (`catalog.json`, read in 2 KiB pieces: it may be 512 KiB) at
    once and asks `core.net` for a fresh one once each time it is opened (`catalog.json.part`, kept only if it reads
    as a catalog). A row is the name, what it is (category, styles, author) and where it stands (reading, update,
    installed, how far a download is, else its size). The troubles (loading, no connection, not reachable) say so,
    in the wording of the device's input (`<key>.buttons` where there is no touch); an empty list that says why
    tries again when it is tapped or confirmed.
  - A font's page shows the sentence, what it is, the licence, and the rows of `native_view::rows` (*Download*,
    *Update*, *Stop*, *Read with*, *Use for the status lines*, *Remove*). A font being downloaded shows only *Stop*
    with its progress (`2 / 5`) and the note that the page can be left; another font waits (`fonts-browser.busy`).
  - The pages are rebuilt on `plugins_changed`, on `setting_changed` of the two font settings and after each file,
    so the list and the page never show a font as it was.
  - *Download* / *Update*: each file is saved by `core.net` into `staging/<plugin>/…`, checked by size and SHA-1,
    and after the last one `command.install_plugin(request, Data, staging/<plugin>, update)` asks the host, which asks
    the user (decision 132). Each download has its own request number (16 up); `install_done` ends a job only when it
    is that job's own, asked question, and no *Stop* is offered once the host was asked. Only an OK outcome says
    *ready*; any other end says *could not be downloaded*; the staging folder goes in every case. *Remove* is
    `command.remove_plugin(2, id)` (the host asks the user, decision 124); a refusal (Denied/Full) says it failed.
  - *Your own fonts*: the text is a message of the plugin (`fonts-browser.own.text`, tested equal to the part of
    `how/` after its heading), written as `own.txt` into the data folder and handed to `core.reader` with
    `command.open` (hand-over `root` 0, `path`; the reader opens it as root 5, `docs/abi/document.md` §2.1).
  - Without a network (`DeviceCapability::Network`) a download says *No connection*. In Offline mode (power level
    Offline, not online) the list with no list yet, and a download, ask once, in place of the rows: tap or Confirm
    goes online (`command.go_online`, 300,000 ms, `uses.wifi`) and goes on; Back leaves. The list also takes a request
    that ended `offline` as that offer.
  - The restart's cancel of a download cut off: `make` removes the `staging` folder (no job can exist then), and a
    download clears its own staging first. Interface 6 cancelled the stored job at `started`; native keeps no job on
    the storage, so there is nothing else to cancel.
- Metadata: `uses.install`, `uses.network`, `uses.wifi`; services `core.device`, `core.net`, `core.registry`, `core.ui`.
- `how/`: kept for the plugin library's page and the handbook (`tools/publish`, `tools/handbook.py`); the native
  plugin has no `HOW` table to give the host, so *Your own fonts* reads its own message, not the help.

## What disappears with interface 6 (for the owner to decide)

Interface 6 is the only form of this plugin the shells that do not activate native plugins can run. Until a host
activates native plugins (`Kernel::register_native`, #944), in the desktop emulator, the Android and web shells and
the tests' kernels there is **no fonts browser at all**: no *More fonts* or *Your own fonts* where the reading font and
the status font are chosen, no font library setting in *Developer* (the `boox-developer-settings` golden lost that
row), and the store's *Discover fonts* says that *More fonts* is not installed. The X4 firmware with `static-native`
links the native plugin. Compared with the interface-6 plugin that is activated, these functions are **not** carried:

1. **The font's own face**: the list showed each name in its own font and the page showed the sentence in it, from a
   small sample pack made by the font library on request (`samples/` of the data folder, held in memory within a
   budget). Native shows the names and the sentence in the menus' font. Named step below (FONT-runtime).
2. The offer in Offline mode was two buttons (*Go online*, *Not now*) giving the network for the screen until it
   closed; native has one choice (tap or Confirm goes online, Back leaves) and asks for 300,000 ms of network.
3. *Remove?* was asked by the plugin with *Remove* / *Keep*; the host asks it now (decision 124), in its words.
4. The download job was kept on the storage (`install.txt`) and begun anew from the font's page after a restart; the
   job is in memory now (a restart drops it and its staging folder, the same outcome).
5. *Update*, *Remove* and the choices of the reading font and the status font are as before; the plugin no longer
   reads the installed fonts from the kernel's plugin list but from `core.registry` (up to sixteen pages of rows).

## Named steps (what is missing, exactly)

- **FONT-runtime (the font's own face):** the host has no way to load a font face at run time. Faces are declared in
  the plugin's metadata (`resource.font_face`, `docs/abi/records.md` record 9, a path in the plugin's own image) and
  `font.acquire` takes a declared key or role. The sample packs would be files in the plugin's data folder after
  `make`. The piece needed is a host call (or a registration accepted after `make`) that makes a face from a file
  of the plugin's Data folder, with a release; then `samples` returns (budget in memory, one sample at a time).
  `tests/acceptance_pending.rs` case `fonts-browser-samples`.
- **Host admits `event.install_done` (97):** `native_host::events::is_event_tag`
  (`crates/paperpaw-core/src/native_host/events.rs`) does not name it, so the host refuses the plugin's event batch
  with `Unsupported` and `make` fails in a real registered scope (`tests/hosted_construction.rs` pins
  `Factory(-1)`; its positive case is ignored until the host admits the tag). Everything the plugin declares apart from
  that is admitted. Without 97 the plugin cannot learn that an install or removal ended.
- **H2 (the host's questions):** the install and removal questions (decisions 132, 124) are not wired to native
  plugins, so no end-to-end run of *Download*, *Update*, *Remove* (`fonts-browser-install`, ignored); the plugin takes
  `Unsupported` / `Denied` as an answer.
- **H1/H4 (screens and input):** native screen commands and input are not qualified on a host
  (`fonts-browser-screens`, ignored).
- **The reader's root 5 (draft 25):** the plugin hands `core.reader` the file of *Your own fonts*; a host answers
  `ERR_UNSUPPORTED` for `doc_open.root` 5 until it is wired, and then the reader shows why it cannot open it. The
  plugin side is done and is not a named step of its own.
- Unmeasured: whether `PowerInfo::online` is set by `command.go_online` (the offer shows again otherwise, once per
  tap); whether the host's `remove` takes the non-empty `staging` folder (an error is ignored, the leftover then goes
  with the next download of that font); X4 timing of checking a 1 MiB font file (SHA-1 in 4 KiB pieces) in one
  callback.

## Review fixes (kept)

- Only `install_done` with status OK and reason 0 says *ready* (`native_plan::ended`); a refused, dismissed or
  timed-out confirmation and any failure say nothing of the kind, and the `staging/<plugin>` folder is removed in
  every case.
- A font the plan cannot install (a file over 512 KiB for packs or 1 MiB for font files, a size of 0, a path that
  leaves the folder, a plugin id over 40 bytes) is not offered (`native_plan::fits`, used by `Catalog::finish`).
- The catalog is read with `Parser::try_feed`/`try_finish` (bounded: 64 levels, 4,096-byte strings, 256-byte keys); a
  refused allocation ends the read in `Error::NoMemory`; the refusal sweep covers the parser as well as the catalog.
