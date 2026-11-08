### Added

- **`CommandSender::install_plugin` and `remove_plugin`** (#228). A native plugin with `uses.install`
  asks the host to install a downloaded plugin (`InstallRoot::Data` or `Files`, update and
  older-version flags) or to remove an installed one; the host asks the user (decisions 124 and 132)
  and ends the request with `install_done`.
