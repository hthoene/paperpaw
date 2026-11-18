### Added
- `core.ui` ops 18 `clipboard_write` and 19 `clipboard_read` (interface 7): the kernel's one clipboard as text 0 of at most 4,096 bytes on every device, written in parts and replacing the old text only when committed, read in parts by any plugin. A committed write is copied out to the system clipboard where the shell has one, and interface-6 plugins read the same text.
