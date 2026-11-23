//! The first-run setup: a short guide through the settings that matter,
//! one a page, each with a good default already chosen.
//! Plugins add its steps (`Staging::add_setup_step`): a setting
//! to choose, or a screen of their own (e.g. Wi-Fi's networks), with
//! whether it is asked on this device. The guide is a screen bound to the
//! slot [`slots::SETUP`](crate::builtin::slots::SETUP), so it can be
//! replaced as a whole.
