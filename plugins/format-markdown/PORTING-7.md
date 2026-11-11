# Interface-7 port

Status: **not complete** (ticket #228). Skeleton: the plugin has its native entry (`declarations.rs`, `NativePlugin`,
`native/metadata*`, `tools/plugin-files7/src/bin/format-markdown.rs`) over the shared provider of `crates/paperpaw-epub-family`
(the converter, the parsers and `core.format` provider of the five plugins of the former `format-epub` crate; its
`PORTING-7.md` has the full notes and gaps). The interface-6 `Plugin` impl is still there, in that crate, and is the
one the app registers; the removal (native only, held until native activation) is a PR of its own per format.
The plugin's texts (`i18n/`, `how/`) live here; the crate that has the interface-6 registration includes them.
Version 1.0.5 (as the crate before the split; no further bump).
