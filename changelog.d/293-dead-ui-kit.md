### Changed

- **SDK: kernel-side ui kit items nothing calls are gone (#293, step S4a).** Since the old plugin API's callers left (S1–S3), parts of the kernel-only `ui` kit (number editor, address view, offline offer buttons, unused heights and draw helpers) had no caller in any crate or feature; they are removed. Interface 7 and api.txt unchanged.
