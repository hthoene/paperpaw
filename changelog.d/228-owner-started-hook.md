### Changed

- **A started plugin gets its services' recovery** (#228). When the kernel takes a native plugin with `uses.network` in, each wired service is told (`ServiceEvents::owner_started`): `core.net` empties the plugin's `.received/` folder and retries a staged file kept after a failed rename, `core.sockets` retries its kept staged files. Before, only the next claim or the plugin's stop did.
