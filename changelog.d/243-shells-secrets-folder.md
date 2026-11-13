### Changed

- **Secrets key folder per platform** (#243, Decision 172). The emulator's secrets key lives in `%APPDATA%` on Windows (the user's own folder, user-only by default) and in `~/Library/Application Support` on macOS, as before in `$XDG_CONFIG_HOME` or `~/.config` elsewhere; on macOS the folder's and the key file's owner is checked against the process's user id.
