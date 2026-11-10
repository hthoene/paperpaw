### Added

- **Core host side of `core.files@1`** (#228). Folders, file information,
  moves and free space (`docs/abi/files.md`): `list`, `stat`, `space`,
  `make_dir`, `remove_tree` and `move` over the callback's storage view, with
  root 0/1/2 rules, paging by a 64-bit cursor, the `ERR_BUSY` rules against
  the library's move job and the host's paths in use, and a family that stays
  `ERR_UNSUPPORTED` until a shell wires its host view. `paperpaw_platform::tree`
  adds `remove_tree_step`, a folder tree removed a bounded piece at a time (at
  most 64 removals and 256 listed entries per call, every visit counted) over
  any `Storage`, without heap, and `kind`; `Storage::remove_listed` removes an
  entry by its place for names no path can hold (FAT overrides it).
