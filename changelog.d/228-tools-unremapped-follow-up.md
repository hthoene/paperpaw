### Fixed

- **Store baseline builds** (#228). `tools/plugin-files/versions.py` no longer passes `PAPERPAW_ALLOW_UNREMAPPED` on to `build.sh`, so the files it hashes against the release's are always built with the paths remapped. Tests also refuse a spaced checkout and a spaced `CARGO_HOME` in both plugin-file builds.
