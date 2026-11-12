### Added

- **One switch for the X4's nested-callback stack gate** (#250). `stage.run`, `ui.compose`, the widget ops 7 and 12-15 and op 14 stay closed on the X4 (`ERR_UNSUPPORTED`) behind one check, `ui_store::nested_closed`. The new paperpaw-core feature `x4-nested` opens all of them: for the owner's X4 test image while the device measurement runs, then for the X4 release once it passes. Off by default; nothing changes for any other device.
