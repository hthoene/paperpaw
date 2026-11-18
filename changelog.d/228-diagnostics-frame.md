### Changed

- **Diagnostics keeps its big buffers off the stack** (#228). The device reads and the step results of the device check now live in memory the plugin asks the host for, and the heavy parts of showing a screen are separate calls, so the plugin's event handler needs well under the X4's stack allowance (it was over it, and the plugin could not be packed). The check behaves as before.
