### Fixed
- **A plugin check says which interface revision a file needs** (#228). `dev_check.needs_draft` (op 6) is the header's `needs_minor` for a plugin file the loader refuses as needing a newer minor (reason 10), and 0 for every other result, as `device.md` 5.1 says; a check no longer reports a file that the loader will refuse at the next start without the number the store needs to say why.
