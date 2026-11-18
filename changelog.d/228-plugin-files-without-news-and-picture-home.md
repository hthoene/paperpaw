### Fixed

- **`tools/plugin-files` no longer lists News and Picture home.** Their interface-6 form is gone, so the build had no binary for them and the firmware-build check "the store's plugins fit the firmware" failed on a missing `news` file. Their store files come with the interface-7 store build. The path test uses a plugin that is still listed.
