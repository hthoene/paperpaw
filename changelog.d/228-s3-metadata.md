### Added

- **Native plugins build for the X4 Pro and X4 Classic** (#228). Each of the 40 interface-7 plugins carries its ESP32-S3 (Xtensa, target 9) ABI metadata profile (`native/metadata.s3.*`, from `tools/abi-metadata.py`) and picks it when built for Xtensa; Xtensa builds enable `asm_experimental_arch` for the SDK's heap word.
