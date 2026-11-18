//! What comes over USB in the main loop: the cable transfer's lines for
//! the kernel, and the test remote control's commands (`remote` builds).

use super::Firmware;
use crate::now;
use crate::usb_power::Sense;
use log::info;

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
                Ok(line) if line.starts_with(paperpaw_core::DEVELOPER_PREFIX.as_bytes()) => {
                    self.developer_line(&line);
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
        self.developer_watch();
        #[cfg(feature = "remote")]
        if self.state.held.is_some_and(|(_, at)| at <= now()) {
            self.release_held();
        }
    }

    /// A line of the developer link (#12): the kernel's rules decide what is
    /// done and what is said (`Kernel::developer_line`); this only carries
    /// the line in and the answers out, in every build.
    #[inline(never)]
    fn developer_line(&mut self, line: &[u8]) {
        let line = alloc::string::String::from_utf8_lossy(line);
        let on = self.kernel.developer_on();
        if on {
            self.state.cable_at = Some(now().0);
            self.wake_dozing();
            // One line a command, never what a file holds.
            if !line.starts_with("DEV/1 ") || line.split(' ').nth(2) != Some("data") {
                info!("{}", line.trim_end());
            }
        }
        let at = self.kernel_now();
        let heap = (esp_alloc::HEAP.used(), esp_alloc::HEAP.free());
        let effects = self.kernel.developer_line(at, &line, heap, &mut |answer| {
            crate::power::feed();
            esp_println::println!("{answer}");
        });
        self.apply(effects);
    }

    /// While the developer link is on: it ends when the cable is pulled or
    /// it is quiet too long, and the computer is told why it ended.
    #[inline(never)]
    fn developer_watch(&mut self) {
        if self.kernel.developer_on() {
            let at = self.kernel_now();
            let effects = if self.usb_power.cable() {
                self.kernel.developer_poll(at)
            } else {
                self.kernel.developer_close(at, "unplugged")
            };
            self.apply(effects);
        }
        if let Some(why) = self.kernel.take_developer_notice() {
            esp_println::println!("DEV/1 0 closed {why}");
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
