### Changed

- **One shared wipe for passwords and typed secrets** (#228). `paperpaw_ui_kit::mem::wipe` and
  `wipe_text` overwrite a buffer in a way the compiler cannot drop; the keyboard and the Wi-Fi
  policy use them instead of a copy each.
