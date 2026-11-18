### Added

- **Decoding hex without an abort** (#228). `paperpaw_digest::hex` gains
  `decode_into` (into the caller's buffer, no allocation), `decode_array`
  (exactly `N` bytes on the stack) and `try_decode` (a vector reserved
  fallibly, so a full heap gives an error). Keys, signatures and digests in
  the SDK, Store and Transfer, and the cable's data lines, use them; a data
  line too long is refused before anything is reserved.
