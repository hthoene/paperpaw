//! What comes over USB in the main loop: the cable transfer's lines for
//! the kernel, and the test remote control's commands (`remote` builds).

use super::Firmware;
use crate::now;

/// Lines of the cable transfer read in one turn of the main loop at most.
const CABLE_LINES: usize = 32;
/// After a line of the cable transfer, the main loop reads on without
/// resting for this long.
const CABLE_QUIET_MS: u64 = 300;

impl Firmware {
    /// What came over USB: the cable transfer's lines for the kernel (all
    /// that is there, a few at most), the test remote control's commands
    /// (`remote` builds, one at a time).
    #[inline(never)]
    pub(super) fn usb(&mut self) {
        for _ in 0..CABLE_LINES {
            let Some(line) = self.usb.next_line() else {
                break;
            };
            match line {
                Ok(line) if line.starts_with(paperpaw_core::CABLE_PREFIX.as_bytes()) => {
                    self.state.cable_at = Some(now().0);
                    // The computer talking to the X4 wakes a doze, so that
                    // what the line does (a frame, a plugin's reply) is
                    // not drawn over the sleep picture.
                    self.wake_dozing();
                    let line = alloc::string::String::from_utf8_lossy(&line);
                    let at = self.kernel_now();
                    let effects = self.kernel.cable_line(at, &line);
                    self.apply(effects);
                }
                #[cfg(feature = "remote")]
                line => {
                    // Only a command wakes a doze, not a line too long.
                    if line.is_ok() {
                        self.wake_dozing();
                    }
                    self.remote(line);
                    break;
                }
                #[cfg(not(feature = "remote"))]
                _ => {}
            }
        }
        #[cfg(feature = "remote")]
        if self.state.held.is_some_and(|(_, at)| at <= now()) {
            self.release_held();
        }
    }

    /// Whether the cable transfer's lines are coming (one came a moment
    /// ago, or one is half there).
    pub(super) fn cable_moving(&self) -> bool {
        self.usb.in_line()
            || self
                .state
                .cable_at
                .is_some_and(|at| now().0 < at + CABLE_QUIET_MS)
    }
}
