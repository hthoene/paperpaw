### Fixed

- **A plugin's own pictures show again** (#277). A plugin installed from the store that asked for a picture it ships (reading goal's cats) was told the card failed: the picture pump did not know the plugin's install folder (root 2 of `command.image_prepare`). It now reads `plugins/<id>/<path>` for the plugin that asks, and only for that one; a plugin built into the firmware has no such folder, so its request ends as "not found".
