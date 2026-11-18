### Changed

- The log line for a native plugin call the host dropped now names the plugin (`native: linked: <id>: call <tag> ended with <status>` on the X4, `native plugin <id>: …` in the emulator). The shell's report function gets the plugin id as a borrowed `&str`, so nothing is allocated (#228, #250).
