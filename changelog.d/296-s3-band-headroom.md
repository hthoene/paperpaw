### Changed

- **The S3 band's `allow indirect-in` ceilings carry a headroom** (#296). The line holds the count of the last CI scan; the scan adds 16 for `paperpaw_plugin_*` and 8 for any other crate, and prints the exact count per crate, so branches counted apart no longer fail the train by a few calls.
