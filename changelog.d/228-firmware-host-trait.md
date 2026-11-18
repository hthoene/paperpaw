### Added

- **`FirmwareHost`, the platform's side of checking, installing and restarting into a firmware image** (#228). One bounded step per call (a check that reads the image in pieces and streams the plugins it has built in, a write of one 64 KiB block at a time bound to the confirmed digest, a read-back, a commit, a restart), with reasons by the numbers of `docs/abi/device.md` 5.5; `FakeFirmware` scripts it for tests and the emulator, with a failure at any call of any stage. The X4's implementation and the kernel's use of it follow.
