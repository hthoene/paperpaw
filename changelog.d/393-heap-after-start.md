### Changed

- **Less memory held at Home after a start** (#393). The plugins that may sleep and were woken only by the start or by reading the library's book details (a format, the file browser) sleep again once that work is over, instead of keeping their state until the device goes to standby.
- **Linked plugins' settings matched faster at the start** (#393). Matching a declared setting with the plugin's built-in table now compares the setting's id first, before any list of choices.
