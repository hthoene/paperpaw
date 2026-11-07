//! What the X4's loader does between placing a format-5 plugin in flash and
//! calling its `make` (`shells/xteink/src/native/ppn5.rs`, `x4::run`): the
//! steps that can still be refused, in an order that leaves nothing behind.
//!
//! A file refused before `make` leaves nothing behind (the module's
//! promise): the flash region it was placed in goes back to the layout, and
//! the image is not listed. So the steps that allocate come first (the id's
//! copy, the device's table of handles, this plugin's share of it), and the
//! image is listed last, with room already reserved by its own step: a
//! refusal at any step gives the region back, once, and an entry that was
//! never pushed has nothing to remove. After `make` the layout is never
//! rewound (the code may be referenced by what the plugin made).
//!
//! The order is the loader's; the firmware implements [`PreMake`] with its real
//! allocations, and the tests here with a record that fails on demand.

/// The refusable steps before `make`, each of which may allocate.
pub trait PreMake {
    /// What a refusal says to the plugin manager.
    type Refusal;

    /// Copies the plugin's id (the list's owned name).
    ///
    /// # Errors
    /// No room.
    fn copy_name(&mut self) -> Result<(), Self::Refusal>;

    /// Makes the device-wide table of handles, if this is the first plugin.
    ///
    /// # Errors
    /// No room.
    fn table(&mut self) -> Result<(), Self::Refusal>;

    /// Takes this plugin's counted share of the table.
    ///
    /// # Errors
    /// No room.
    fn share(&mut self) -> Result<(), Self::Refusal>;

    /// Lists the placed image (reserving its place in the list, then pushing
    /// without allocating): the only step that makes the image visible.
    ///
    /// # Errors
    /// No room; nothing is listed then.
    fn list_image(&mut self) -> Result<(), Self::Refusal>;

    /// Gives the flash region back to the layout (the placed plugin is
    /// forgotten as if it had not been placed).
    fn give_back_region(&mut self);
}

/// Runs the steps in order. A refusal at any step gives the region back,
/// exactly once, and stops: nothing later ran, and no image is listed.
///
/// # Errors
/// The first step's refusal.
pub fn prepare<P: PreMake>(steps: &mut P) -> Result<(), P::Refusal> {
    let done = (|| {
        steps.copy_name()?;
        steps.table()?;
        steps.share()?;
        steps.list_image()
    })();
    if done.is_err() {
        steps.give_back_region();
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Default)]
    struct Record {
        /// Which step refuses (0-based, in order), or none.
        fail_at: Option<usize>,
        calls: Vec<&'static str>,
        listed: u32,
        given_back: u32,
    }

    impl Record {
        fn step(&mut self, n: usize, name: &'static str) -> Result<(), &'static str> {
            self.calls.push(name);
            if self.fail_at == Some(n) {
                Err(name)
            } else {
                Ok(())
            }
        }
    }

    impl PreMake for Record {
        type Refusal = &'static str;
        fn copy_name(&mut self) -> Result<(), &'static str> {
            self.step(0, "copy_name")
        }
        fn table(&mut self) -> Result<(), &'static str> {
            self.step(1, "table")
        }
        fn share(&mut self) -> Result<(), &'static str> {
            self.step(2, "share")
        }
        fn list_image(&mut self) -> Result<(), &'static str> {
            self.step(3, "list_image")?;
            self.listed += 1;
            Ok(())
        }
        fn give_back_region(&mut self) {
            self.given_back += 1;
        }
    }

    #[test]
    fn all_steps_run_in_order_and_the_region_stays_placed() {
        let mut r = Record::default();
        assert_eq!(prepare(&mut r), Ok(()));
        assert_eq!(r.calls, ["copy_name", "table", "share", "list_image"]);
        assert_eq!((r.listed, r.given_back), (1, 0));
    }

    #[test]
    fn the_id_copy_failing_gives_the_region_back_and_lists_nothing() {
        let mut r = Record {
            fail_at: Some(0),
            ..Record::default()
        };
        assert_eq!(prepare(&mut r), Err("copy_name"));
        assert_eq!(r.calls, ["copy_name"]);
        assert_eq!((r.listed, r.given_back), (0, 1));
    }

    #[test]
    fn the_device_table_failing_gives_the_region_back_and_lists_nothing() {
        let mut r = Record {
            fail_at: Some(1),
            ..Record::default()
        };
        assert_eq!(prepare(&mut r), Err("table"));
        assert_eq!(r.calls, ["copy_name", "table"]);
        assert_eq!((r.listed, r.given_back), (0, 1));
    }

    #[test]
    fn the_table_share_failing_gives_the_region_back_and_lists_nothing() {
        let mut r = Record {
            fail_at: Some(2),
            ..Record::default()
        };
        assert_eq!(prepare(&mut r), Err("share"));
        assert_eq!(r.calls, ["copy_name", "table", "share"]);
        assert_eq!((r.listed, r.given_back), (0, 1));
    }

    #[test]
    fn the_image_list_failing_gives_the_region_back_with_nothing_listed() {
        let mut r = Record {
            fail_at: Some(3),
            ..Record::default()
        };
        assert_eq!(prepare(&mut r), Err("list_image"));
        assert_eq!(r.calls, ["copy_name", "table", "share", "list_image"]);
        assert_eq!((r.listed, r.given_back), (0, 1));
    }
}
