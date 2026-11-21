### Fixed

- **X4 and X3 sleep between page turns without crashing** (#434, #451). The
  processor could crash on waking from its light sleep (the X3 logged a false
  "Stack overflow", the X4 restarted), most often on the first nap. It now
  sleeps on its crystal and sets its fast clock up again after each wake, as
  Espressif's own SDK does. On the X4 a cable plugged in wakes it at once, so
  the computer finds it ready; its battery switch is never held in a sleep,
  which is what made other firmware hang on plugging in.
