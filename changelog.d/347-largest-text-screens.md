### Fixed

- **Largest text: screens that were cut or drawn over themselves** (#348).
  At the largest text size the keyboard's letters were higher than their
  keys on the X4 Pro and the Boox Go 7 and *123* wider than its key; the
  text of *Wi-Fi transfer* without a network ran under the button
  *Without a router*; the words of an empty list (*News* without feeds)
  ran past the bottom of the screen; the store's two texts lay over each
  other while the list loads (German); and in the quick panel a long
  title lay under its value. Letters are now as large as their key
  holds, texts keep to the lines their room holds and end in "…", and
  the panel's title ends before the value. At the usual text size nothing
  changes.
- **`paperpaw-dev visual`, `reach`, `gallery` and `snapshots` look at
  every screen again** (#347). After the firmware confirmation was shown
  it stayed open, every later screen opened below it and was left out
  without a word: the checks, CI's too, saw 5 of 77 screens. Going back
  to the home screen now closes a screen that guards the device too. The
  focus check passes over rows that do nothing, and a screen that hides
  the row of hints is checked against the whole display.
