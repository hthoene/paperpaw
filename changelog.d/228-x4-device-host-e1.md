### Added

- **`core.device` on the X4, wired** (#228): interface-7 builds make the device once at start (`shells/xteink/src/device_host.rs`: identity and `caps` per device, the controls the kernel is built with, the start, the running firmware, what code plugins stand on, the plugins not installed, the memory probe), wire it behind the host table with `core.ui`, tell it the code room when the loader is done, and offer `core.device` to admission. Nothing runs on the X4 until the stack gate lifts; the Wi-Fi, power, Bluetooth and USB readings follow in the next part.
