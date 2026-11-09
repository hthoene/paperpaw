### Changed

- `paperpaw-sign pack` for the ESP32-S3 (target 9): a plugin may now have a few
  words in a literal pool that nothing reads, the toolchain's dead constants
  (at most 64 bytes, only when no relocation, stored address, call, jump or
  branch reaches them); the packer says which. The packer also skips hidden
  global symbols in its export check. Every other uncovered byte between
  functions is still refused.
