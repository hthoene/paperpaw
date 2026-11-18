### Fixed

- **Plugin-file builds** (#228). `PAPERPAW_ALLOW_UNREMAPPED=1`, the override for a trial build in a path with a space, is never read for a build with `--key` or `--fingerprint` (release, CI, store), so it cannot leak into a published file. `tools/plugin-files7/build.sh` refuses a spaced path as `tools/plugin-files/build.sh` does, with the same override. A test runs both scripts against a stand-in `cargo`.
