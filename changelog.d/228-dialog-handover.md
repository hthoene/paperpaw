### Fixed
- Interface 7: the dialog of a question (the keyboard for a text) opens with the hand-over the host makes for it, holding `core.request` and `core.kind` (`docs/abi/ui.md` 3.2). Nothing made it, so a native keyboard opened with none and refused (`Invalid`): no text could be asked for on the native plugins.
- Interface 7: the request of a question is open for its dialog from the start of the open, so the keyboard reads its question while it opens (it was `State` until the open returned).
