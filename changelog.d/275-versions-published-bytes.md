### Changed

- Tooling: `versions.py --published releases.json` compares the folder of each data plugin with the
  `<id>-<version>.zip` a release published (GitHub's list of the releases), and fails when the bytes differ under
  the same version, whatever a `Plugin-unchanged:` trailer says; the message names the next patch version (#275).
  `literata-large` 1.1.0 changed its store text without a new version and was not caught by the commit check.
- Tooling: the release page's text is `docs/release-notes/<version>.md` (`tools/check-release-notes.py`: UTF-8, not
  blank, at most 125,000 characters, GitHub's limit). A tag release stops in its first job without it and again in the
  last one; a dry run warns and shows the CHANGELOG section (#275).
