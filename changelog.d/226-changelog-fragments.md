### Changed

- **Changelog entries stay with their pull requests** (#226). Each change
  adds a small file in `changelog.d`, so parallel pull requests no longer
  edit the same release section. Release preparation collects the files
  into `CHANGELOG.md`; a lightweight check reports missing entries.
