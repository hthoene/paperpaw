### Added

- X4: a firmware built with `native-on` hosts the linked native plugins for
  real (`register_native_plugins`, offering exactly the services it wired),
  in place of their interface-6 forms. A test image (`test-image`) also wires
  `core.secrets` with a veiling sealer (not encryption; Wi-Fi, settings and
  the lock keep their passwords) and reports `secrets-veiled` in the device
  values; the `release` feature refuses it at compile time and a check
  refuses it in the release build.
