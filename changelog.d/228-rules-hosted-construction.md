### Changed

- **Rules on the new plugin interface: constructed in the host's real scope** (#228). The host's
  limit of callback registrations per plugin is 32 now, so the rules plugin's nine event
  subscriptions are admitted and it is constructed there without refusal.
