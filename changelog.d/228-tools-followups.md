### Fixed

- **Tools** (#228). `tools/plugin-files/catalogue.py` refuses a plugin file shorter than its header (a format-4 file of 12 to 15 bytes raised a raw `struct.error`) with a clean message. `tools/flash-gate.sh` refuses a `CARGO_HOME` with a space (its path remaps would be split) and removes the records of its own killed runs' worktrees, no one else's. `tools/plugin-files/build.sh` refuses to build when the checkout, target or cargo home path has a space (the files would depend on where they were built); `PAPERPAW_ALLOW_UNREMAPPED=1` allows a trial build.
