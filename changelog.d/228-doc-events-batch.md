### Fixed

- **`event.document_changed` and `event.document_closed` reach the opener** (#243). They were handed over as a bare record, which a callback refuses before the plugin sees it; they are the one-record batch every event is. A book prepared at its first open no longer stays at "Opening the book for the first time".
