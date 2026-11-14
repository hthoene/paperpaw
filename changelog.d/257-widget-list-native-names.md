### Fixed

- **The editor of widget lists names the widgets of interface-7 plugins** (#257). The sleep
  screen's widget list showed raw ids (`home.clock`) and offered only spaces to add, since the
  kernel's editor knew the widgets of interface-6 plugins only. It now reads the names and kinds
  of the widgets native plugins declare. No interface change (`shape.txt`: one private field).
