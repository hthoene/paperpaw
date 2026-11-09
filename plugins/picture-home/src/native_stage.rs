//! Staging what a change of the pages writes: the settings, the placements
//! reset, the message. All of a change's writes are staged in one callback, and
//! an error return drops them all (`screens-input.md` 2.1), so a change is all
//! or nothing.
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, ResetTarget},
    settings::SettingValue,
    ui::Notify,
};
use paperpaw_ui_kit::{Messages, mem::buffer};

use crate::native_pages::{self, Write};

/// Room for a command besides its text.
const OVERHEAD: usize = 512;

/// Stages the settings written and the placements reset. Over the host's budget
/// for one callback nothing is staged: the old layout stays, the reader is told,
/// and `false` comes back.
///
/// # Errors
/// Allocation and host errors; an error return drops everything staged.
pub fn apply(host: &Host<'_>, writes: &[Write], resets: &[String]) -> Result<bool, Error> {
    if !native_pages::fits(writes, resets) {
        say(host, "picture-home.too-big")?;
        return Ok(false);
    }
    let longest = writes
        .iter()
        .map(|write| write.text.len())
        .max()
        .unwrap_or(0);
    let mut scratch = buffer(OVERHEAD + longest)?;
    let mut sender = CommandSender::new(host, &mut scratch);
    for write in writes {
        sender.set_setting(&write.setting, SettingValue::Text(&write.text))?;
    }
    for id in resets {
        sender.reset_setting(ResetTarget::Placement(id))?;
    }
    Ok(true)
}

/// Says the message of the catalog key `key`.
///
/// # Errors
/// Allocation and host errors.
pub fn say(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text = Messages::new()?.copy(host, key)?;
    let mut scratch = buffer(OVERHEAD + text.len())?;
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}
