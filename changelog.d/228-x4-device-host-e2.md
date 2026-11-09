### Added

- **`core.device` on the X4 follows the kernel** (#228): Wi-Fi (state, address, the networks a scan found), Bluetooth (radio, bonded devices), the power level and mode, the USB readings and the front-light percent are told to the device where the firmware already learns of them (after each step of the kernel, a scan, the status), so plugins read what the kernel keeps and nothing polls. The Wi-Fi and Bluetooth `caps` bits are set with their setters. `Kernel::wifi_model` and `Kernel::bluetooth_model` give a shell what plugins see of both.
