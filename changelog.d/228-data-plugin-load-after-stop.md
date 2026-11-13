### Fixed

- **A data plugin whose install finishes during a sleep or a plugin's stop loads at once too** (#244, #228). When the device went to sleep, or the plugin that asked stopped, while an install swapped the new version in, the swap finished in the stop's bounded steps but the plugin was loaded only at the next start, with the old version still loaded over the new files. It now loads there as after any other install.
