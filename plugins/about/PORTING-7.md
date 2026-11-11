# About: porting notes (interface 7)

## A missing fact never stops the page (#243)

`read_facts` treated only `NotFound` as "the device has none" for the firmware, the built-in plugins and the
native facts. The desktop shell wires no backend for them, so the host answers `Unsupported` and the first
callback ended -1 (the draws after it -6, no screen). `Unsupported` now leaves the row out like `NotFound`
(as Beta 3 on the desktop), and so does a refused `about.lines` stage run. Test:
`tests/hosted_open.rs` (the screen opens and no callback is dropped).
