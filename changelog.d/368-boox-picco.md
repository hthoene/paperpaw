### Added

- **The Boox Picco, experimental** (#368). A firmware for Onyx's Boox Picco
  comes with each release: `paperpaw-picco-<version>-install.bin`, flashed
  over USB into the stock firmware's app partition, which leaves Onyx's boot
  loader and recovery as they are. It is built from what CrossPoint Reader
  and the FreeInk SDK found out about the device and has **not been tried
  on a Picco**: it may show nothing or not start. Save the Picco's own
  flash first, the only way back to its stock firmware
  (`shells/xteink/README.md`). It is an X4 Pro with another board: touch,
  the front light, two page buttons, Wi-Fi and Bluetooth; no update from
  the device itself (one app partition), and the store's plugins are the
  X4 Pro's. The emulator has it as the profile `boox-picco`.
