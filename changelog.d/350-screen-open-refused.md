### Fixed

- **A screen whose plugin refuses the open no longer stays open and blank** (#350). Where a plugin's `screen_opened` ends with an error (a screen opened by another plugin with wrong parameters), the kernel closes the screen again and shows the notice "This screen could not be opened." instead of leaving an empty screen on the stack that every draw failed on.
