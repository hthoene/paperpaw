### Fixed

- **Plugins learn how long the device slept** (#483). `core.device` op 1 now reports the time asleep that the kernel itself measured from suspend to resume. Before, it reported "unknown" on every device, because no shell kept a figure of its own.
