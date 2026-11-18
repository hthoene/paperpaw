### Added

- **The X4's side of firmware check, install and restart** (#228). `paperpaw-x4`'s `Updates` implements the platform's `FirmwareHost`: a check that streams the plugin ids an image has built in, hashes and verifies it (the signature, or another firmware's image as the boot loader would), a write bound to the digest the user confirmed (reason 18 where the image changed), a read-back, and a commit that starts a PaperPaw image on trial and makes another firmware's final. `paperpaw-update` gets `PluginCrate::plugin_id`. Interface 6's flow is as it was.
