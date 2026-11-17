### Changed

- **The editor of a list of widgets is the sleep plugin's; the kernel's own editors are gone (#293, step S5b).** The sleep screen's widgets are edited on the sleep plugin's screen `sleep.edit-widgets` (slot `core.edit-widgets`), drawn as native screens draw, with the same rows, options and grips. The library folder's editor is the folder chooser's slot itself, and the kernel's unused status items and lines choosers are removed. Interface 7 and api.txt unchanged.
