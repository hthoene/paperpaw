### Fixed

- **Notices show with native plugins** (#257). Messages such as "Saved" or an error (`command.notify`, the kernel's own) vanished when the native menu replaced the interface-6 one, because only the latter filled the notice slot. The host now has its own notice screen, `core.notice-box` (Beta 3's look: a dark rounded box above the bottom, at most three lines, gone after `core.notice-seconds`), bound to `core.notice` as a fallback; a plugin's notice screen still wins.
