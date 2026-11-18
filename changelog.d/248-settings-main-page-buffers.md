### Changed

- **Opening Settings holds fewer buffers at once** (#248). The check for passwords and keys now runs through the registry reader's own buffers and before the list of groups is built, instead of allocating a second 5.4 KiB pair beside them; the list of secrets asks for a 256-byte query buffer, not 1,280.
