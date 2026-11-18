### Added

- **Staged annotation edits in the native SDK** (#228). `Library::{edit_begin,
  edit_add, edit_commit, edit_abort, edit_receipt}` with `Transaction`,
  `AnnotationEdit` (add, upsert, replace, remove), `Committed` and the receipt pages
  (`ReceiptPage`, `Receipt`, `ReceiptAction`): long texts, notes and XPointers are sent in parts,
  every limit is checked before the host is called.
