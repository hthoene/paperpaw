# About: porting notes (interface 7)

## A missing fact never stops the page (#243)

`read_facts` treated only `NotFound` as "the device has none" for the firmware, the built-in plugins and the
native facts. The desktop shell wires no backend for them, so the host answers `Unsupported` and the first
callback ended -1 (the draws after it -6, no screen). `Unsupported` now leaves the row out like `NotFound`
(as Beta 3 on the desktop), and so does a refused `about.lines` stage run. Test:
`tests/hosted_open.rs` (the screen opens and no callback is dropped).

## Licences and the plugins built in open in the reader again (#459)

Both rows refused (`Unsupported`, "blocked on `doc_open.root` 5"); the hand-over of a plugin's own
file has been in the host since. As in Beta 3, the page writes the text to its data folder
(`licences.md`, `plugins.md`, `src/native_pages.rs`) and hands it to the reader (root `0`,
`command.open` into `core.reader`). `build.rs` packs the build's licences (firmware or app) in
deflated parts of at most 8 KiB, as Beta 3 did (the firmware's 55,201 B are 8 parts, about 17 KB
packed, by zlib level 9); each part is inflated and appended on its own. The plugins' page is read from `core.device` when it is chosen
(id and version). Not ported: Beta 3's logo at the top of the licences.
