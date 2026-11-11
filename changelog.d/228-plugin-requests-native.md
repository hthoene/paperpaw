### Added

- Native plugins can ask for plugins that are not on the device (`command.request_plugins`): the host holds at most four open requests, tells installers that subscribed with `event.plugins_requested`, and answers `failed` at once when no installer can answer.
