### Added

- **`CommandSender::install_firmware` and `firmware_restart`** (#228). A native plugin with
  `uses.firmware` asks the host to install a firmware image from its data or the user's files
  (`FirmwareOptions`: allow an older version, a foreign image, the signing key of an image signed by
  another key). The host always asks the user and ends the request with `install_done` (kind 1).
  `firmware_restart` starts the image the plugin's own install left waiting; the host asks again,
  and declining keeps the image installed for the next start.
