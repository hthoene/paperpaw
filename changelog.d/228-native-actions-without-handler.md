### Changed

- An interface-7 plugin's action is kept without a handler of its own: `paperpaw_sdk::NativeAction`
  (id, title, plugin and token, with `Actions::insert_native` and `Action::native`) in place of an
  `ActionSpec` with a boxed handler; the kernel queues `event.action_run` where the action runs,
  with a warning where its plugin is gone or disabled. A native screen keeps its id once. Less
  heap per native action and screen on the X4 (#248).
- `Kernel::owned_native_texts` lists interface-7 plugins' ids and titles kept as heap copies, with
  `paperpaw_sdk::Id::kept` and `Str::kept` (kernel feature); a `paperpaw-app` test holds every
  linked plugin to none. Power's id table now also names its actions and screens (#248).
