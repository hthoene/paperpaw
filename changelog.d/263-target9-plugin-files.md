### Added

- **Interface-7 plugin files for the X4 Pro and X4 Classic** (#263). `tools/plugin-files7/build.sh x4-pro` and `x4-classic` pack every plugin as a target-9 file (format 5, Xtensa) and check it; the release, the store workflow and CI build, check and sign the S3 files beside the X4's. Each plugin's S3 metadata is generated (`tools/plugin-files7/s3-metadata.py`).
