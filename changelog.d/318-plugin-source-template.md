### Added

- **A template for a community plugin source** (#318).
  `templates/plugin-source` is a repository to copy: an example plugin with
  code for plugin interface 7, and a GitHub Actions workflow that builds it
  for the Xteink X4 and X3, the X4 Pro and X4 Classic and the desktop
  emulator, signs the builds and the list with the source's key (the
  repository secret `PAPERPAW_SOURCE_KEY`) and publishes the source with
  GitHub Pages. Its README says how to make the key and how readers add the
  source on their device (*Plugin store*, *Sources*).
- **`paperpaw-sign pack-source`, `sign-source` and `check-source`** (#318).
  `pack-source` makes a source from the folders of its plugins and their
  signed builds: `plugins.json`, `plugins.json.sig`, a page and the files of
  each plugin, as any web server serves them. It refuses what a device would
  not install, such as a build signed with another key than the list.
  `sign-source` signs a list made another way; `check-source` checks a
  list's signature as a device does and prints the key's fingerprint.
