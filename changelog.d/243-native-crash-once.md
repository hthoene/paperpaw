### Fixed
- A built-in native plugin that crashed is held back for that start only and stays switched on in `paperpaw.ini` (`NativeDecision::CrashedOnce`): when it is the only plugin of a role the device needs (plugin manager, settings, menu) it could be turned on again only by editing the file on a PC. A crashed card plugin is still switched off for good.
