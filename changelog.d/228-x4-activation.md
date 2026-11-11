### Added

- X4: a firmware built with `native-on` hosts the linked native plugins for
  real (`register_native_plugins`, offering exactly the services it wired),
  in place of their interface-6 forms. The test image (`test-image`) also logs
  the stack's and the heap's high-water marks every 30 s and keeps the lows
  through a reset; the `release` feature refuses it at compile time and a check
  refuses it in the release build. A start that fails twice in a row, and a band
  lost after a sleep, go on without native plugins.
