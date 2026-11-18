### Added

- **The X4 Pro and X4 Classic watch the end of the main stack** (#228). Where interface-7 plugins are built in, a 2 KiB band above a 14 KiB reserve at the stack's end is watched by the chip's debug assistant: a plugin (or firmware code a plugin called) that overflows the stack ends in the crash slot blaming that plugin, not in a failure somewhere else later. Code plugins run only where the band armed at start (and again after a light sleep). The 16 KiB come out of the internal heap on those two devices.
