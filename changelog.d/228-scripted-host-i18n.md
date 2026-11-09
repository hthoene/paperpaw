### Added

- **The scripted test host answers `i18n.copy`, `i18n.format` and `i18n.digits`** (#228). A native plugin's callbacks that build texts no longer end in `ERR_UNSUPPORTED` in tests: feed the host the plugin's catalogs (`ScriptedHost::add_catalog`, `add_catalogs_in` for its `i18n` folder) and a language (`set_language`), and the slots answer with the shell's own code (`native_host::formatting::{copy_with, format_with, digits_with}`), so keys, fallback to English, plurals, placeholders and digits give the real results. The three slots can be refused and counted like the others.
