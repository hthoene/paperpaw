### Changed

- CI and `tools/check-local.sh` (new step `test-native`) run the app's `flows`, `sleep_parity` and `sleep_cycles` with `--features native-linked` (#248): the workspace test run leaves the feature off, so they had never run there. `check-local` runs the step when the core, the app, the native layer or a plugin changed, or with `--all-linked`.
