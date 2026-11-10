### Changed

- **The sleep screen draws its pictures natively** (#228). The sleeping cat, the logo above the
  message, the cover of the book read last, the next picture of the folder and a picture over the
  page are now decoded and drawn by the host through the plugin's image requests, as the
  interface-6 screen drew them. Until a picture is ready the message shows and the screen
  refreshes when it comes. The folder and the card pictures need the optional storage consent;
  without it the cat shows. A cover that is still being made also shows the cat.
