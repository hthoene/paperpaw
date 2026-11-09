### Changed

- **`sleep` binds `core.sleep` as Kernel ruled** (#228). At start and when the plugins change, a stored foreign mode binds the slot unless the reader has bound it, and nothing is unbound; on a change of `sleep.show` (or a reset) it binds an offered mode and unbinds only a slot bound to another plugin's sleep mode. A screen the reader bound stays across restarts.
