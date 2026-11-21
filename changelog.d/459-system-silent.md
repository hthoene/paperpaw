### Fixed

- **Screen lock: a code that could not be saved said nothing** (#459). On the X4 with Wi-Fi off, the PIN was typed twice and then nothing happened. The lock now says the code could not be saved and to turn Wi-Fi on, and offers the kinds again; unlocking with an older code no longer fails when its renewal cannot be saved (lock 1.0.7).
- **Settings: Reset all did nothing when many settings were changed** (#459). With more than 63 changed settings nothing was reset and nothing was said. Such a reset now says to reset page by page (settings 1.0.12).
- **Setup stayed partly in English after choosing German** (#459). Next, Back, Skip and the last page follow the language chosen on the first page at once, as in Beta 3 (setup 1.0.5).
- **Transfer: files over 64 MB failed without a reason** (#459). The page says such a file is too large, with the limit, before sending it (transfer 1.0.7).
- **Plugin manager: "How it works" was missing for eleven plugins** (#459): Progress sync, wallabag, Readwise, Joplin, Wikipedia, File sync, Browser, Read later, RTF, Edge light and Picture frame show their text again.
- **Store: Offline mode only said "No connection"** (#459). Where Offline mode holds the list back, the store offers to go online for it again, as in Beta 3, and a tap or Select goes online and loads it (store 1.0.16).
