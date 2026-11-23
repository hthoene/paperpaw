### Fixed

- X4 and X3: the start log no longer warns of an "unknown setting" for settings of the plugins built into the firmware (such as `sleep.*` and `wifi.while-charging`): the warnings are logged once these plugins have declared their settings, so only settings nobody knows are reported (#509). The values were always applied.
