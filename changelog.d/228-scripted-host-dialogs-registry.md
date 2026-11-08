### Added

- **The scripted host answers questions and pages lists** (#228). In
  `paperpaw-scripted-host` (test support), `ScriptedHost::dialogs()` is a
  scripted `core.ui` for questions: request ids, the request text in parts,
  the caps of a text and of a secret, answers by kind and the
  `event.answered` batch for the asker; `ScriptedHost::registry()` is a
  scripted `core.registry` that pages its rows by cursor and generation like
  the host; `sweep_released` runs a refusal sweep and fails a path that leaves
  a heap block or a service handle held.
