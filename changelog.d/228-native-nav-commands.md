### Added

- Native plugins can now use the navigation commands of interface 7 on the kernel's own screen stack: `close` of an instance (own screens and other plugins' overlays), `back_to`, `set_key_layer`, `show_hints` and `raise_input`. A guard's rules (only its plugin closes or raises past it) and the 8-hop chain limit apply; the root screen is never closed. `screen_policy` stays unsupported.
