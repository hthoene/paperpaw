### Fixed

- Another plugin's screen chosen as the sleep screen (the flash card in *Sleep screen*) is shown when the device sleeps: the host now takes the sleep plugin's `bind_slot` command, a choice added by a plugin that registers after the sleep plugin is no longer dropped from `paperpaw.ini` at start, and the vocabulary's sleep card notes that it is shown (#354).
