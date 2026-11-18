### Fixed

- **No plugin can write a user grant** (#228). A command that sets, resets or previews a `core.grant.*` setting is refused as denied, whole batch with it, for every plugin including the one the grant names and the settings editor, whether or not such a setting is registered. Before, an unregistered grant key was refused as invalid and a registered one was treated as an ordinary setting.
