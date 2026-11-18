//! The restart and start screens plugins choose (slots `core.restart`,
//! `core.start`, #219): the restart screen drawn, or *Restarting* where a
//! plugin's does not come in time; the start screen drawn ahead onto the
//! card for the next start. The power goes in any case.

use core::sync::atomic::{AtomicBool, Ordering};

use log::info;

use super::Firmware;
use crate::now;

impl Firmware {
    /// The restart screen a plugin chose, drawn whole and waited for; the
    /// device's own *Restarting* where none is bound, it did not open or its
    /// opening took longer than the kernel's time limit.
    /// A restart asked again while that screen is drawn draws nothing more.
    #[inline(never)]
    pub(super) fn restart_screen(&mut self) {
        static DRAWING: AtomicBool = AtomicBool::new(false);
        if DRAWING.swap(true, Ordering::Relaxed) {
            return;
        }
        match self.kernel.restart_screen(now(), &now) {
            Some(effects) => {
                info!("restart display: the plugins' screen");
                self.apply(effects);
                self.settle_frame();
            }
            None => crate::startup::restarting(&mut self.panel, &self.kernel),
        }
        self.prepare_start_picture();
    }

    /// The start screen a plugin chose, drawn onto the card for the next
    /// start (removed where none is chosen): nothing on the panel changes.
    #[inline(never)]
    pub(super) fn prepare_start_picture(&mut self) {
        let begun = now();
        let prepared = self.kernel.prepare_start_picture(begun, &now);
        info!("start picture: {prepared:?} in {} ms", now().since(begun));
    }
}
