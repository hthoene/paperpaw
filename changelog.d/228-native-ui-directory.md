### Added

- Native widget/status declarations use the shared retained declaration store and
  full-width host owner identities. Callback frames expose the directory, bounded
  owner/token status calls and a fixed kernel stack snapshot. UI cleanup retries
  borrowed stores after callbacks return; X4 nested UI activation remains gated (#228).

### Fixed

- Placed setting reads apply grant and secret visibility to the canonical setting
  identity and reject nested placements. Deferred UI cleanup retains all closed
  instances for retry, and weak directory teardown preserves memory safety (#228).
