### Changed

- **The sleep screen is native only on interface 7** (#228). `sleep` shows its screen (the message,
  the widgets you chose, a widget over the page, the page itself, the cat, the cover and the pictures of
  the folder) from a native entry, closes when the device wakes and takes the presses while asleep as
  before. The picture and the widget over the page are picked on a screen of their own: the pictures
  folder, its pictures, the pictures other plugins offer, or a widget, with a note of why the last
  picture did not show. The interface-6 plugin is gone. The clear parts of a PNG or a GIF over the page
  stay opaque and a picture named under another plugin's folder is not shown until the host allows both;
  *A flash card due* is not offered as a sleep screen until Vocabulary's own port declares it.
