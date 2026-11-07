### Added

- Power is a native interface-7 plugin only (1.1.2): the interface-6 plugin and its Kernel
  tests are removed; the mode choice, the power menu and the sleep-screen-first ordering move
  to a host-free model with tests, the DEV commands (sleep, power off, restart, wear) have
  typed SDK methods, and the dialog's answer reaches the plugin without a subscription (#228).

### Changed

- **Shorter help for "Sleep after" and "Record battery use"** (#228). Both explanations now fit
  their two lines at the largest text size on the X4 Pro, so they show whole (en and de).
