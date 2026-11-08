# Interface-7 port (slice 1: the model and the declarations)

Version 1.0.7. Partial: what the plugin declares, the catalog and the install plan without the host.
No native screen and no `NATIVE_ENTRY` yet; the interface-6 `FontsBrowser` and everything of it
stays until the host activates native plugins (then it goes together, no shim).

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

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

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
- `paperpaw_json::stream::Parser` allocates infallibly (bounded: 64 levels, 4,096-byte strings,
  256-byte keys). Everything this plugin allocates is swept; the parser is not. Asked of Kernel.

## Review fixes

- Only `install_done` with status OK and reason 0 says *ready* (`native_plan::ended`); a refused,
  dismissed or timed-out confirmation and any failure say nothing of the kind, and the
  `staging/<plugin>` folder is removed in every case.
- A font the plan cannot install (a file over 512 KiB for packs or 1 MiB for font files, a size of
  0, a path that leaves the folder, a plugin id over 40 bytes) is not offered
  (`native_plan::fits`, used by `Catalog::finish`).
