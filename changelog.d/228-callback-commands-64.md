### Changed

- **A callback may stage 64 commands** (#228). The per-callback command limit
  of the native command path is the named `CALLBACK_COMMANDS` (64, was 8), so a
  profile apply fits one callback; the 4,096-byte budget of copied keys and text
  and the wire batch cap are unchanged. The 65th command, and bytes over the
  budget, are `ERR_NO_SPACE` with nothing more staged; a callback that returns
  an error still drops the whole stage.
