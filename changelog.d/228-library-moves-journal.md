### Added
- The library's move journal on the card (`.paperpaw/library/journal`): one checksummed record, written to a temporary file, synced and renamed over the journal, so a power cut leaves the old state or the new one; a record that cannot be read is an error and is never overwritten. First slice of the production library moves.
