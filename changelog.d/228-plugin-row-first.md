### Fixed

- **A plugin is not loaded without its row in the plugin list** (#248). The kernel copies a plugin's manifest for the list before anything of the plugin is registered; where there is no room for that copy, the plugin is not loaded (with a warning), instead of running unlisted, which could have let a second copy load or an update install afresh.
