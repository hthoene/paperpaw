### Fixed

- **Native screens: every button has a name** (#243). The settings' stepper buttons are told as "−" and "+", and the native home's footer buttons by their names (the pencil as *Edit* or *Done*, *Add* as "+", an action by its name), as interface 6 told them, so reading aloud and automation find them. ui-kit gains `paint::describe_named` for parts that show an icon or a sign and no words.
