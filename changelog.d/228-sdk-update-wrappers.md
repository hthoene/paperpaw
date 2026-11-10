### Added

- **`Device::leaves_out` and `CommandSender::set_item_state`** (#228). `leaves_out` lists the built-in plugins the last finished firmware check's image leaves out (device op 16), 32 rows a page. `set_item_state` with `ItemState` (hidden, faint, a switch, keep, a catalog label and badge with `formatting::Arg` arguments) says what a menu shows of one of the plugin's own items (command 60). The update plugin's "update without them" choice and its New mark use them.
