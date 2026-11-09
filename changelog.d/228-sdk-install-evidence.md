### Added

- **A plugin sends the catalog's trust evidence with an install** (#228). `CommandSender::install_plugin_with` sends the author and recovery keys and the rotation statements in the same batch as the install; `CommandSender::trust_revoked` hands the host revocations to keep. The host still applies every trust rule itself. An install carries at most 6 rotations (a batch holds 8 commands); revocations are sent 8 to a batch.
