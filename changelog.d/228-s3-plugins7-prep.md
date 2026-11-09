### Changed

- **Plugin library entries read interface-7 files** (#228, decision 142). `tools/plugin-files/catalogue.py` writes `abi = "7.<needs_minor>"` for a format-5 file (header bytes 8..12, decimal) where a format-4 file keeps its fingerprint, checks the file's chip against its device, and names `tools/plugin-files7` as the source of such a build. `tools/plugin-files7/build.sh` builds for the X4 Pro and X4 Classic too (compile only: it reports "pending" until the target-9 metadata profile exists), and CI runs it.
