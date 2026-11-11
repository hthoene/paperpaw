### Added
- `event.screen_ready` (87) and `command.screen_ready` (1557) for native plugins (interface 7): a plugin can register for 87 and is told, once for each screen instance, after the instance's first draw; a screen declared `waits_ready` says so itself with the command (its own top instance, else `ERR_STATE`; a repeat raises nothing). An instance closed before it was drawn is never told.
