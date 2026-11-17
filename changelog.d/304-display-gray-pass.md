### Changed

- **A key ends the gray pass, and every screen after grays is drawn while the panel is cleaned** (#304). A key pressed while a picture screen is drawn in grays no longer waits for the 1.3 s of drawing and the 1.1 s refresh: the pass stops, and the next screen is drawn whole. After grays the next screen (any, not only Home) is drawn while the panel is cleaned to white, and shown with a fast refresh.
