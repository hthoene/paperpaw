### Changed

- **The widget pack keeps its state until something needs it** (#248). What the pack holds (its widgets, the to-do list, the cached quotes, numbers, books, pictures and the reading recorder, about 1 KiB) is made by the first widget, screen, question, answer or book event, not when the pack is made; a tick, a dropped widget, a changed setting or the end of a session find nothing to do without it. Behaviour is the same.
