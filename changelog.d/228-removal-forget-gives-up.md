### Fixed
- A plugin removal with its data whose secrets the host cannot forget no longer keeps its journal, and with it every later install, waiting for ever: after three starts in a row the removal is finished without forgetting (the secrets stay on the card), it is logged and the user is told (en/de). The grants of a removed plugin are all forgotten, not the first eight.
