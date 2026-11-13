### Fixed

- **Emulator: a native plugin's HTTP answer no longer waits forever** (#2795). After the desktop
  shell tells core of an answer (`core.net`) or a socket report (`core.sockets`), it ticks the
  kernel once, as the X4's shell does: the kernel's pass is what tells the plugin. A download whose
  screen waited only for its next answer stayed at "1 / 4".
- **A plugin that asks for pictures without subscribing to `event.image_ready` is warned** (#2795):
  its answers are dropped as the contract says, now with one runtime warning naming it.
