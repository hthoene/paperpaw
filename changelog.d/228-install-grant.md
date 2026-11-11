### Added

- **A plugin's right to install without asking is the user's setting** (#228). The host reads `core.grant.install.<plugin>` (off by default) just before it steps a plugin's install, so a grant given or revoked counts from the next step on. A plugin cannot write the setting, reads only its own, and a plugin that is not running holds no grant.
