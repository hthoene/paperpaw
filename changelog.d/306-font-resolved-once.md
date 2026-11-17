### Changed

- **Text is set faster** (#306). The font of a role (menus, book text, status)
  is looked up once and remembered until the fonts change, and a text
  draw no longer looks its font up twice. Every text measure and draw
  is cheaper, so pages and Home draw sooner.
