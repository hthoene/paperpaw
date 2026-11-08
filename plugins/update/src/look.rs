//! Looking for updates by itself (the setting [`LOOK`], off unless the
//! reader turns it on, in the first-run setup or the settings): about
//! once a week the update server is asked, as the page asks it, but only
//! at a cheap moment, when the radio is on anyway or the device charges
//! (declared work, ADR 0025: optional, without a deadline, so it never
//! switches the radio on by itself on the battery). What it finds waits
//! quietly where the reader looks: the update page, and a badge on its
//! entry in the settings; nothing is said over reading.

use core::time::Duration;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::work::{Moments, Work};
use paperpaw_sdk::{Command, Commands, Env, Storage};

use crate::{file, online};

/// Whether the device looks for updates by itself.
pub const LOOK: Setting<bool> = Setting::new("update.look");
/// Declared work: the weekly look.
pub(crate) const LOOK_DUE: Event<()> = Event::new("update.look-due");
/// When it looked last, in seconds since 1970, where the clock was known.
const LOOKED: &str = "looked.txt";
const WEEK: u64 = 7 * 24 * 3600;

/// Declares the weekly look, after what is left of the week since the
/// last one, or takes it back where the setting is off. Declared again it
/// replaces itself.
pub(crate) fn plan(env: &Env<'_>, out: &mut Commands) {
    if env.firmware().is_none() {
        return;
    }
    let due = LOOK_DUE.id().clone();
    if !env.settings().read(&LOOK) {
        out.push(Command::CancelWork(due, None));
        return;
    }
    let now = paperpaw_sdk::time::unix_seconds(env);
    let after = wait(looked(env.storage()), now);
    out.push(Command::work(
        Work::network(due)
            .optional()
            .after(Duration::from_secs(after))
            .every(Duration::from_secs(WEEK))
            .at_moments(Moments::ONLINE | Moments::CHARGING),
    ));
}

/// How long to wait before looking, with `last` the last look and `now`
/// the time, where known: the rest of the week; a week where the clock
/// is not known now but a look was (not again at every start).
pub(crate) fn wait(last: Option<i64>, now: Option<i64>) -> u64 {
    match (last, now) {
        (Some(last), Some(now)) => WEEK.saturating_sub(now.saturating_sub(last).max(0) as u64),
        (Some(_), None) => WEEK,
        (None, _) => 0,
    }
}

/// The work is due: the server asked, quietly.
pub(crate) fn due(env: &Env<'_>, out: &mut Commands) {
    if !env.settings().read(&LOOK) {
        return;
    }
    let storage = env.storage();
    if let Some(now) = paperpaw_sdk::time::unix_seconds(env) {
        let _ = storage.create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
        let _ = storage.write(&file(LOOKED), alloc::format!("{now}\n").as_bytes());
    }
    online::check(env, true, crate::count::Trigger::Auto, out);
}

fn looked(storage: &dyn Storage) -> Option<i64> {
    let text = paperpaw_sdk::read_file(storage, &file(LOOKED), 64).ok()?;
    core::str::from_utf8(&text).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_waits_the_rest_of_the_week() {
        let day = 24 * 3600;
        assert_eq!(wait(None, Some(1000)), 0);
        assert_eq!(wait(None, None), 0);
        assert_eq!(wait(Some(0), Some(2 * day)), WEEK - 2 * day as u64);
        assert_eq!(wait(Some(0), Some(30 * day)), 0);
        assert_eq!(wait(Some(0), None), WEEK);
        // A clock set back: a whole week, never more.
        assert_eq!(wait(Some(10 * day), Some(day)), WEEK);
    }
}
