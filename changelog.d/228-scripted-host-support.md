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
