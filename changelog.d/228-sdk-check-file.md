### Added

- **`CommandSender::check_file`** (#228). A native plugin with `uses.install` or `uses.firmware`
  asks the host to check a plugin or a firmware image before the user is asked to install it
  (`CheckKind::{Plugin, Firmware}`); the check ends with `install_done` kind 3 and its evidence is
  read with `Device::check`.
