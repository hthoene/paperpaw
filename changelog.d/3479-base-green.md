### Changed

- **The S3 band names four registry lookups of the kernel env change** (#300, #3479): `resolve_slot`, `Screens::find`, `find_native` and `find_fixed` each have one call through a register, and the frame-lifetime probe declares the kernel's `Place<'a>`.
