### Fixed

- **Less X4 stack under a native plugin's callback during a tick** (#250). The kernel's tick, its
  end-of-call work, the dialog pump and the command loop keep their locals in frames of their own,
  so they are off the stack while a plugin's callback runs below them (the route of a nested
  plugin callback on the X4): about 3.9 KB less from `Kernel::tick` to the plugin call.
