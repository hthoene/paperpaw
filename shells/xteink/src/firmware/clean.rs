//! The panel cleaned to white while the next frame is drawn (#267).
//!
//! After grays (a book's cover page, in four grays) every pixel has to be
//! driven back, a half refresh of 1.7 s. Home's first frame after Back draws for
//! 1.5 s on the X4 before it is sent, so drawing and half refresh took 3.2 s one
//! after the other. The panel is cleaned to white first, the next screen (any,
//! #304) is drawn into the kept bands meanwhile, and the frame is shown with a fast refresh (0.5 s)
//! from white.

use log::{info, warn};
use paperpaw_platform::{FrameView, PixelFormat};
use paperpaw_x4::clean;
use paperpaw_x4::display::{Planes, Refresh};

use super::Firmware;
use crate::now;

/// White, as a band of rows (1 bit a pixel, 1 is white).
static WHITE: [u8; clean::WHITE_BYTES] = [0xFF; clean::WHITE_BYTES];

impl Firmware {
    /// Whether the frame to show after grays is cleaned while it is drawn.
    pub(super) fn cleans_while_drawing(&self, after_gray: bool, half: bool) -> bool {
        clean::wanted(
            self.panel.cleans_while_drawing() && !self.panel.keeps_frame(),
            after_gray,
            half,
        )
    }

    /// Writes white into both memories and starts the half refresh that shows
    /// it, without waiting. `false` where either failed.
    pub(super) fn start_clean(&mut self) -> bool {
        let started = now();
        if self.write_white(Planes::BOTH).is_err() {
            return false;
        }
        let ok = self.panel.start_refresh(Refresh::Half, false).is_ok();
        info!("clean to white started in {} ms", now().since(started));
        ok
    }

    /// Writes white over the whole screen into `planes`.
    pub(super) fn write_white(
        &mut self,
        planes: Planes,
    ) -> Result<(), paperpaw_x4::display::Error> {
        let (width, height) = self.panel.size();
        let stride = usize::from(width).div_ceil(8).max(1);
        for (top, rows) in clean::bands(width, height) {
            let data = WHITE.get(..usize::from(rows) * stride).unwrap_or(&WHITE);
            let view = FrameView::new(width, rows, PixelFormat::Mono1, stride, data);
            if let Err(e) = self.panel.write_rows(top, view, planes) {
                warn!("display: {e:?}");
                return Err(e);
            }
        }
        Ok(())
    }
}
