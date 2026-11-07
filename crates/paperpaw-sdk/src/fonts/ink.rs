//! Where the ink of text lies as it is drawn ([`FontHandle::ink_drawn`]):
//! measured by drawing it on a probe, so that shaping, joining, marks,
//! ligatures and jamo can never set it otherwise than the drawing does.

use paperpaw_graphics::{Framebuffer, InkBounds};
use paperpaw_platform::Color;

use super::FontHandle;

impl FontHandle<'_> {
    /// The ink of one line of `text` at `size` exactly as
    /// [`FontHandle::draw`] puts it on a frame that shows a mask's pixel
    /// inked from coverage `inked_from` (the frame's
    /// [`Framebuffer::ink_threshold`]: its black-and-white threshold, or
    /// half covered), with its pen at 0 and its baseline at 0
    /// (`left`/`top` inclusive,
    /// `right`/`bottom` exclusive, above the baseline negative). `None` for
    /// text without ink. Keeps no pixels: it costs the glyphs drawn, from
    /// the glyph cache as drawing does, and no memory of its own. Also
    /// while the registry only describes (glyphs are made then).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn ink_drawn(&self, size: f32, text: &str, inked_from: u8) -> Option<InkBounds> {
        let mut probe = Framebuffer::probe(inked_from);
        // Drawing while describing measures only; the probe needs glyphs.
        let describing = self.drawing.describing.replace(false);
        self.draw(&mut probe.canvas(), size, 0.0, 0, text, Color::BLACK);
        self.drawing.describing.set(describing);
        probe.probed()
    }
}
