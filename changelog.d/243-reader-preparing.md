### Added

- **`event.document_changed` in the SDK** (#243, #2443): `EventKind::DocumentChanged` subscribes an opener to the host's hint that a document it opened moved in its preparation (`document.md` 2.1), and `Event::DocumentChanged` (`kinds::DocumentChanged`) reads its document, state and progress. A state the SDK does not know reads as failed, as `DocState::preparation` does.

### Fixed

- **Books opened for the first time open in the native reader** (#243, #2443). A book whose format prepares it first (DOCX, MOBI) was closed at once while the reader said "Opening the book for the first time", so its preparation never ran and the book never opened. The reader now keeps the document while it is prepared, opens the book when the host says it is ready, says it could not when it failed, and gives the document back once, when the screen closes, also when the reader leaves while it is still prepared. A reading screen shown again asks the state itself, so a lost hint never leaves it waiting.
