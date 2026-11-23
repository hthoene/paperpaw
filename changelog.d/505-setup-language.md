### Fixed

- **Setup speaks only the language chosen** (#505). With the language changed
  while the first-run setup was open, a step its plugin had just hidden kept
  the title of the language before: an English setup on the X4 Classic showed
  the store's step as "Mehr hinzufügen". Every step takes the new title now.
  `paperpaw-dev` tells text of another language on a screen (`sweep.text`) and
  walks the setup with the language changed while it is open
  (`lint.language-mixed`).
