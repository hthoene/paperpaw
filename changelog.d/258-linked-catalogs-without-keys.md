### Changed

- The translations of the plugins built into the firmware take less flash: the program keeps a
  hash of each text's key instead of the key itself, once for English and German together, and
  none of the files' comments (#258). Texts read the same; a plugin's own translation files and
  the languages on the card work as before.
  `paperpaw_i18n::BuiltMessages` (made by `BuiltMessagesTable` in a build script, handed to
  `Translator::built_messages`) is the new form; `paperpaw_app::NATIVE_MESSAGES` holds the
  linked plugins' catalogs, `Kernel::add_built_messages` adds them
  (`paperpaw_app::add_native_catalogs` does it).
