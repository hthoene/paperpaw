### Added

- **A scripted host for native plugin tests** (#228). The new, unpublished
  `paperpaw-scripted-host` crate makes a plugin from its entry with a host
  table it answers itself, runs its event, input and draw callbacks, and
  shows what the plugin did: the kernel's own command staging (limits, drop
  on error, no stage in a draw), scripted `core.*` service answers,
  `settings.read` and `env.snapshot` values, `Answered` events, and refusals
  (`ERR_FULL`, `ERR_BUSY`, `ERR_NO_MEMORY` ...) at the n-th call of a slot with
  a sweep over every call. `CALLBACK_COMMANDS` and `CALLBACK_COMMAND_BYTES`
  are public so tests can name the stage's limits.
  Statuses follow the shell: input above 1 and event results above 0 are
  `ERR_INVALID`, events get no reply buffer, an unset setting reads as the
  encoded `Absent` record, `service.open` of an unknown key is
  `ERR_NOT_FOUND` (one handle per key, else `ERR_FULL`) and `service.call`
  checks the caller's buffer against the op's reply bound before the
  service runs.
