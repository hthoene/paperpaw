### Fixed

- **"Skip setup" leaves the setup assistant again** (#433). The answer of a question reaches the asking plugin as an event, and an event had no screen stack, so its close was refused: the setup assistant stayed on its first page and the setup was never marked done; typing the lock's password and removing a stored secret in Settings did not leave their screen either. The answer's callback may now close the asker's own screen (as an action may), and the dialog's own close is applied before the answer is delivered.
