### Changed

- **Reading a setting is cheaper** (#306). Where a setting is kept is
  looked up once and remembered, and the secret check no longer looks it
  up a second time, so the many setting reads of a screen cost much less.
