### Added

- **Native plugins get `event.tick` once a minute while the device is awake** (#228). The kernel sends one tick a minute to each plugin registered for it, never asks for an extra wake to do so (a tick goes out where the kernel is ticked anyway, and a device that slept for several minutes gets one when it wakes), and does not filter ticks by screen or widget. A plugin registered only for another event (`started`, `resume`, ...) gets no tick record.
