### Fixed

- **A native screen opened with params by anything but a native plugin gets them** (#243). A shell's script, an interface-6 plugin or the kernel opening a native screen with params (for example the choice of how the screen is turned, `settings.choose` with `id=core.rotation`) handed nothing over, so the screen opened empty. The host now hands the params over itself (op 16's caps, `text` also as the hand-over's text), freed when the screen closes; a native plugin's own `open` is unchanged.
