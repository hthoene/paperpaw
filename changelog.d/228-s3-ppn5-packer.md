### Added

- **Target 9 for format-5 plugin files** (#228, decision 142). `paperpaw-sign` packs linked Xtensa plugins (X4 Pro, X4 Classic) with a frame scan of the windowed ABI (frames under 2,048 B, no `call0`), `paperpaw-native` reads the target-9 header profile, and the X4 Pro and X4 Classic firmware has a PSRAM placement path. The path stays inert until the stack gate has a design for the S3: the loader refuses every format-5 file there with `Unsupported`.
