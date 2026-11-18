### Added

- **The host answers `text.draw_inked`** (#228). Slot 41 draws for native plugins: no canvas is `ERR_STATE`, then a stale font handle, then the arguments (a tone of 0 or 16 bytes, no unknown flag bits). A widening above 16 is clamped to 16, `sharp` applies to the chosen curve, and a refused argument draws nothing; as with `text.draw`, an advance error is reported after drawing.
