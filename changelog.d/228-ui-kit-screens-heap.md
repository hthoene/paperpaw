### Changed

- **Open screens no longer hold spare room** (#248). `ui_kit::Screens` grew its lists to four rows at
  the first screen; with a reader's view of about 900 bytes that was 3.6 KiB held for one book. It now
  grows exactly (`mem::push`), so a plugin with one open screen holds one row.
