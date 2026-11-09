### Added

- **Plugin secrets in the native SDK** (#228). `native::secrets::Secrets`
  gives typed owner-only reads, paged editor listings, save and remove calls
  and random bytes with a strength flag. Replies borrow the caller's buffers,
  values and random bytes print redacted, and secret writes clear their encoded
  input on success and refusal. A public volatile `secrets::wipe` clears a
  buffer after use. The facade allocates nothing.
