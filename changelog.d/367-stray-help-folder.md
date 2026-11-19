### Fixed

- A stray `--help/` folder (91 hyphenation pack files) at the repository root is gone: `tools/hyphenation-data.py`, `literata-packs.py` and `script-pack.py` took `--help` for their output folder and now print their usage instead; the quick check fails when a tracked path starts with `-` (#367).
- The plugin store's publisher no longer lists `android` as a device for plugins: Android loads no code plugins (#237).
