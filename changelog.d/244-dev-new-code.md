### Added

- **`paperpaw-dev new --github <id> --code` makes a plugin with code for plugin interface 7** (#244). Beside the repository of a data plugin it writes the crate (`native_plugin!`, a screen, an action in the tools menu), the X4's build configuration with the release's linker script, the record of the device's functions the code calls, and its words; `CODE.md` says how to build it (`cargo x4`). `--sdk <path>` builds against a PaperPaw checkout instead of the published SDK. Before, `--code` only said that there was no template.
