### Fixed

- **The store's download screen ends at 100 %, not at 99 %** (#244). Where PaperPaw takes the end of an install, the screen showed "99 %" until a key was pressed; it shows the end and the next key closes it.

### Changed

- **A chain of plugins that need each other is a question for each step** (#244). The store offers what a plugin needs after the install of the one before.

- **A disabled row reads on the X4's black-and-white screen** (#244). The facts of a plugin's page (version, size, needs) were drawn in a grey that is paper on 1-bit screens; they are black there now.

- **The store page shows the whole summary, "Needs … free", and the store says "no connection" when its list never comes** (#244). Before, the list stayed "Loading plugins…" when the answer came late and was a refusal.
