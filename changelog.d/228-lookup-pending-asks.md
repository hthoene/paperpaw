### Added

- **Pending lookup asks** (#228). The lookup service keeps, in a fixed
  table that allocates nothing, which asker waits for which provider's answer
  when a provider answers `pending`, and ends each ask on the provider's ready
  command, after 60 seconds, or when either plugin stops. The table is 16
  entries on the X4 and 32 elsewhere; a full table answers the empty entry.
