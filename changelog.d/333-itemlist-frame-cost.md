### Changed

- Lists draw faster on the X4 in every plugin: a frame is drawn in bands, and each band used to read the look from the settings, wrap the note under the list and lay out the header again. The first band of a frame now does it once and the other bands of that frame use it (the work of a list's draw is down by about a sixth in the dev tool's measurement). Nothing is kept between frames, and the lists carry less in each plugin's frame than before.
