### Added

- **The kernel has a device log for its own diagnostics** (#228). `KernelBuilder::device_log(sink)` takes one function that gets each line (at most 120 bytes, plugin ids and numbers only, never plugin text, paths or secrets); without it the lines are dropped and nothing is formatted. The host services log through `Frame::log`. The desktop emulator prints the lines to its error stream; the X4 follows with the `log` crate.
