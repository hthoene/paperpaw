### Fixed

- An action sent by a remote command or a test script (not a key press) closes a dialog before the plugin hears the dialog's answer, as a key press does (#266). Before, a screen the plugin opened from the answer was closed in the dialog's place and the dialog stayed on top.
