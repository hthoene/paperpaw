### Added

- **A plugin sends the catalog's trust evidence with an install** (#228). `CommandSender::install_plugin_with` sends the author and recovery keys and the rotation statements in the same batch as the install; `CommandSender::trust_revoked` hands the host revocations to keep. The host still applies every trust rule itself. A batch holds 8 commands, so an install carries at most 6 rotations with a key and 7 without.
