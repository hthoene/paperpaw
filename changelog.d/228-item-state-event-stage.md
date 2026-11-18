### Fixed
- A native plugin's event and action callbacks can set the state of its items (`command.set_item_state`): the stage of those callbacks admits it as a screen's does, and the state is stored when the kernel runs the batch. The update plugin's "open" item was refused (`ERR_UNSUPPORTED`) at start.
