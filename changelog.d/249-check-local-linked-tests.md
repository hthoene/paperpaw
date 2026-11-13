### Changed

- `tools/check-local.sh` has a `test-linked` step: the tests of each selected package that has a `linked` feature run with `--features linked` (the plugins' hosted tests are built nowhere else), and `--all-linked` does it for every such package, which the train smoke uses.
