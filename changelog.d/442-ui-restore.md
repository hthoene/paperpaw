### Fixed

- **Screen lock could not be set by touch; the tick showed a "?"** (#442). In Settings, Screen lock, a tap on PIN, Pattern or Password did nothing on a touch screen (the list answered only the buttons), and the kind in use was marked with a "?" where Beta 3 ticked it. A tap chooses the kind again, and the kind in use is marked with a dot like in other choices.
- **"Reset all" and other questions lost their words** (#442). Settings, Reset all asked only "Yes, reset / Cancel" under the word "Settings": the sentence saying what is reset was missing. A choice shows what its question says again (also the file's name or path where a menu is about a file).
- **Set up PaperPaw showed a key instead of the keyboard layout** (#442). The keyboard step showed `keyboard.layout.auto`; it shows "Automatic (UI language)" again.
