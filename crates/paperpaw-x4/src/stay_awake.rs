//! Staying awake on USB, a developer's choice (#96): while the cable is in
//! and the choice is on, the sleep after *Sleep after* does not come (the
//! kernel holds it, `Kernel::set_awake_on_usb`), and a sleep the reader asks
//! for shows the sleep picture but does not sleep: the X4 *dozes*, awake
//! with USB alive (a light sleep or a deep one cuts the cable's link, the
//! computer loses the X4 until the power button is pressed), and wakes at the
//! next press or command. A cable pulled while it dozes puts it to sleep
//! for real at once, so that a forgotten X4 does not drain its battery. This
//! is the state of it, on a computer-tested machine; the firmware
//! (`shells/xteink`) does the sleeping, the waking and the telling.

/// What the firmware keeps of staying awake.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StayAwake {
    /// The choice is on and the cable is in, as the kernel was told.
    on: bool,
    /// The sleep picture is shown, plugins heard `core.suspend`, and the X4
    /// waits awake for a press or a command.
    dozing: bool,
}

impl StayAwake {
    /// Looks at the choice and the cable (`wanted`: both). What the kernel
    /// is to be told, if that changed.
    pub fn watch(&mut self, wanted: bool) -> Option<bool> {
        (self.on != wanted).then(|| {
            self.on = wanted;
            wanted
        })
    }

    /// Whether the X4 stays awake now: a request to sleep dozes.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// The X4 dozes: the sleep was shown and heard.
    pub fn doze(&mut self) {
        self.dozing = true;
    }

    /// Whether the X4 dozes.
    pub fn is_dozing(&self) -> bool {
        self.dozing
    }

    /// A press or a command came: whether the X4 was dozing, and is awake
    /// again now (plugins hear `core.resume`).
    pub fn wake(&mut self) -> bool {
        core::mem::take(&mut self.dozing)
    }

    /// The cable went while the X4 dozed: it sleeps for real now.
    pub fn must_sleep(&self) -> bool {
        self.dozing && !self.on
    }

    /// A real sleep begins: whether plugins heard `core.suspend` already
    /// (the doze before it), then not again.
    pub fn suspended_already(&mut self) -> bool {
        core::mem::take(&mut self.dozing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_tells_the_kernel_only_what_changed() {
        let mut stay = StayAwake::default();
        assert_eq!(stay.watch(false), None);
        assert_eq!(stay.watch(true), Some(true));
        assert_eq!(stay.watch(true), None);
        assert!(stay.is_on());
        assert_eq!(stay.watch(false), Some(false));
        assert!(!stay.is_on());
    }

    #[test]
    fn a_doze_ends_at_the_first_press_or_command() {
        let mut stay = StayAwake::default();
        stay.watch(true);
        stay.doze();
        assert!(stay.is_dozing());
        assert!(stay.wake());
        assert!(!stay.is_dozing());
        // A second one finds it awake: no second resume.
        assert!(!stay.wake());
    }

    #[test]
    fn a_cable_pulled_while_dozing_sleeps_for_real_without_a_second_suspend() {
        let mut stay = StayAwake::default();
        stay.watch(true);
        stay.doze();
        assert!(!stay.must_sleep());
        assert_eq!(stay.watch(false), Some(false));
        assert!(stay.must_sleep());
        // The sleep takes the doze over: plugins heard core.suspend once.
        assert!(stay.suspended_already());
        assert!(!stay.must_sleep());
        // A later sleep is a plain one.
        assert!(!stay.suspended_already());
    }

    #[test]
    fn a_cable_in_and_awake_is_no_reason_to_sleep() {
        let mut stay = StayAwake::default();
        stay.watch(true);
        assert!(!stay.must_sleep());
        stay.watch(false);
        assert!(!stay.must_sleep());
    }
}
