### Fixed
- An `open` a native plugin stages with a hand-over (the reader's open-last, home's open book, the selected text of a translation) is no longer refused: the stages of the screen and event callbacks take the hand-over store and the callback's stamp, and the hand-overs a callback made that no open claimed are freed when it returns.
