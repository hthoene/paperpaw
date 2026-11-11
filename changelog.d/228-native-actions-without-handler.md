### Changed

- An interface-7 plugin's action is kept without a handler of its own: `paperpaw_sdk::NativeAction`
  (id, title, plugin and token, with `Actions::insert_native` and `Action::native`) in place of an
  `ActionSpec` with a boxed handler, and the kernel queues `event.action_run` where the action runs.
  A native screen keeps its id once. Less heap per native action and screen on the X4 (#248).
