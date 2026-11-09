### Added

- **Work events in the native SDK** (#228). `native::work` decodes
  `event.work_due` (`WorkDue`: key, token, `Why`), `event.work_refused`
  (`WorkRefused`: `Refusal::NetworkOff`) and `event.work_allowed`, as
  `Event::{WorkDue, WorkRefused, WorkAllowed}` and
  `EventKind::{WorkDue, WorkRefused, WorkAllowed}`.
