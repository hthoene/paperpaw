### Added

- **`CommandSender::install_firmware` and `firmware_restart`** (#228). A native plugin with
  `uses.firmware` asks the host to install a firmware image from its data or the user's files
  (`FirmwareOptions`: allow an older version, a foreign image, the signing key of an image signed by
  another key) and later to start the installed image. The host always asks the user and ends the
  request with `install_done` (kind 1).
