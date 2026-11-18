### Fixed

- **A library commit ends the caller's transaction whatever it answers** (#228). `edit_commit` on one's own transaction used to leave it open (until the 10-minute expiry) when parts were missing or the reply did not fit; it now ends it, as the SDK expects, and a later commit or abort on the id answers `ERR_NOT_FOUND`.
