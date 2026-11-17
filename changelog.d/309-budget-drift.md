### Changed

- **The S3 band's per-function `allow indirect` counts are ceilings** (#309). A line allows its count plus 2 calls through a register (LLVM's layout moves a site or two between builds), fewer calls pass, and a stale `allow indirect` or `allow call` line is a warning that the scan prints, no longer an error. Frame sizes, new unlisted sites and every other `allow` line stay exact.
- **The frame-lifetime probe derives the plain types it needs** from the module's own `paperpaw_sdk::` / `paperpaw_native::` paths, so a new memo or cache type needs no line in `tools/tests/test_abi_frame_lifetimes.py`.
