### Added

- **Per-stage subscriber switch** (#228, decision 131). The host setting `core.stage.<stage id>.<plugin id>` turns one plugin off for one stage: it is then not in the list `stage.run` goes through and does not count toward the 32 active subscribers, from the next callback of the stage's owner. It is on unless set to no. The setting has no screen or definition yet.
