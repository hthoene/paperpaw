### Fixed
- Interface 7: an action a native plugin declares (Transfer, About, Bluetooth …) now does what the plugin asks, such as opening its screen, in the call that ran it. The host ran the action's callback without a command stage, so the plugin's request was refused and nothing opened from a button, a rule or a script's `run:`.
