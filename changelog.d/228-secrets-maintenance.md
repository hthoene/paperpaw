### Added

- Shells get `WiredHosts::secrets()` with a startup migration that seals
  interface 6's veiled values in bounded steps (an interrupted step keeps the
  old file) and a plugin-removal hook that forgets a plugin's secrets when the
  user chooses to delete its data (#228).
- A save refused because the device has no strong random source (the X4 with
  its radio off) tells the shell through `SecretsInputs::needs_radio`; the host
  text says to turn on Wi-Fi, in English and German (#228).
