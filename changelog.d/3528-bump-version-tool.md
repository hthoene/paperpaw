### Changed

- **A release's version is set by one command** (#3528). `python3 tools/bump-version.py 0.6.0-beta.N` rewrites the workspace version, the internal requirements, the docs and store texts that name the release, the SDK's user agent and the Android version name and code; the lockfiles are refreshed with `cargo metadata`. The Release workflow's comments name the 81 assets of a firmware release.
