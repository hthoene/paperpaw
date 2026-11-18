### Fixed

- **Less X4 stack under a native plugin's event** (#250). The kernel's event delivery keeps the interface-6 handlers, the guard's screens, the book moves, the work moments and the screens' contexts in frames of their own, so they are off the stack while a native plugin hears the event, the route of a nested plugin callback (`stage.run`, `ui.compose`) on the X4.
