### Added

- Native installer plugins can answer plugin requests (`command.answer_plugin`): only a plugin that declared `uses.install` may, the requester alone hears `event.plugin_answered`, the first answer for a plugin wins, "installed" is checked by the host, and a request is closed when it is fully answered, when no installer is left, or when its requester stops. A request naming `core` is refused.
