### Changed

- Book pages and the screen the device sleeps on are no longer drawn twice where no picture can be on them: the kernel counted the pictures with one more draw of the whole screen before every large frame, also on screens without any. It now skips that count when no screen shown holds a picture of its own; screens with pictures still get their grays as before (#267).

### Fixed

- A question a plugin asks from an action run by the shell (`Kernel::run_action`) now opens its dialog at once, not only at the next unrelated wake-up (#267).
