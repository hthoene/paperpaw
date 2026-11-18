### Added

- **A native plugin's screen can relabel a button in the row of button hints** (#257). `CommandSender::set_hint` with a `native::ui::ButtonHint` sends the existing `command.set_hint`: the plugin's own words (*Try again*), an icon, no hint, or the action's usual name again, while its own top screen shows. No change to the interface: the command was there, the SDK had no method for it.
