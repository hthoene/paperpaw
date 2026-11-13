### Fixed

- **Interface 7, lookup: a provider's `lookup_ready` reaches the asker** (#244). The host staged no `command.lookup_ready` (`ERR_UNSUPPORTED`) and kept no pending ask on a device, so an ask answered `pending` could only end by its deadline. The command is now checked, applied after the callback (a work run's callback may send it), and each subscribed asker gets `event.lookup_ready` from the event pump; the pending asks of a plugin that stops end with it.
