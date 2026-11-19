//! The Boox Picco's front light: cool and warm LEDs behind a driver on the
//! I²C bus the input interrupt reads the touch screen on, so it is set
//! through `input` (`paperpaw_x4::picco` has the driver's registers and the
//! stock firmware's table of brightness and warmth). The settings
//! `x4.frontlight` (how bright, in percent) and `x4.warmth` (how much of it
//! warm) are shown, as on the X4 Pro (`light.rs`).

pub struct Light {
    /// The brightness and warmth shown, in percent.
    shown: Option<(u8, u8)>,
}

impl Light {
    /// The light, off.
    pub fn new() -> Self {
        Self { shown: None }
    }

    /// Shows `level` percent of brightness, `warmth` percent of it warm.
    pub fn show(&mut self, level: i64, warmth: i64) {
        let level = level.clamp(0, 100) as u8;
        let warmth = warmth.clamp(0, 100) as u8;
        if self.shown == Some((level, warmth)) {
            return;
        }
        self.shown = Some((level, warmth));
        crate::input::light(Some((level, warmth)));
    }

    /// Off, e.g. asleep or for want of input (`light_timer`);
    /// [`Light::show`] brings it back.
    pub fn off(&mut self) {
        if self.shown.is_none() {
            return;
        }
        self.shown = None;
        crate::input::light(None);
    }
}
