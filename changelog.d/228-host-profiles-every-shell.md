### Added

- **Interface 7: a host profile for every shell** (#243, owner decision 172). The interface declares five more profiles for linked built-in plugins: Windows x86-64, macOS x86-64 and Apple silicon, Android AArch64 and the web (wasm32). A linked plugin's metadata now names the profile of the target its program is built for (`paperpaw_native::profile::BUILD_PROFILE`), set by the SDK at compile time, so the emulator on Windows and macOS, the Android app and the web emulator can host the built-in native plugins. No card or store file loads on those targets.
