### Changed

- **The plugin list reads linked plugins' manifests from the program** (#248). Interface 7's one shape change for this (owner's Decision 168): `PluginInfo::manifest` is a `ManifestRef`, a manifest kept in the program (`Manifest::fixed`, `ManifestRef::fixed`) or a copy of its own, and it reads as a `Manifest` as before (`info.manifest.id`). `PluginInfo::new` takes either. A plugin's row in the list takes 40 bytes, not 104 (emulator).
