### Changed

- **Transfer and update keep less on the heap while idle** (#248). The transfer plugin's cable screen and update's download, check and offer state are made when they are first needed instead of living inside the plugin's own block from the start (about 0.8 KiB less at idle on the X4). `ui_kit::mem::boxed` is the fallible `Box::new` for plugins that want the same.
