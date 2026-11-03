//! The end signal: what shows that a phase is over besides the notice.
//!
//! - *Notice* (the default): the notice alone.
//! - *Flash the screen*: one full refresh, which e-ink shows as a flash
//!   (and which clears the ghosts of the countdown).
//! - *Blink the front light*: the front light goes to the other end for
//!   a moment and back (where the device has one; else a flash).

use alloc::string::ToString;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::settings::FRONTLIGHT;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::{Command, Env, Params, RefreshMode, SettingValue};

pub const NOTICE: &str = "pomodoro.signal.notice";
pub const FLASH: &str = "pomodoro.signal.flash";
pub const LIGHT: &str = "pomodoro.signal.light";
pub static SIGNALS: [&str; 3] = [NOTICE, FLASH, LIGHT];

/// The front light goes back to what it was.
pub const BLINK_BACK: Event<Params> = Event::new("pomodoro.blink-back");

/// Milliseconds the front light stays at the other end.
const BLINK_MS: u64 = 700;

/// The commands that give the signal chosen in `signal`.
pub fn give(env: &Env<'_>, signal: &str, out: &mut Vec<Command>) {
    match signal {
        FLASH => out.push(Command::Redraw(RefreshMode::Full)),
        LIGHT if env.caps().frontlight => {
            let now = env.settings().read(&FRONTLIGHT);
            let other = if now >= 50 { 0 } else { 100 };
            out.push(Command::SetSetting(
                FRONTLIGHT.id().clone(),
                SettingValue::Int(other),
            ));
            let back = Params::new().with("v", now.to_string());
            out.push(BLINK_BACK.schedule(back, env.now().add_millis(BLINK_MS)));
        }
        LIGHT => out.push(Command::Redraw(RefreshMode::Full)),
        _ => {}
    }
}

/// The command that brings the front light back, from the event's
/// parameters.
pub fn back(params: &Params) -> Option<Command> {
    let value = params.get("v")?.parse::<i64>().ok()?.clamp(0, 100);
    Some(Command::SetSetting(
        FRONTLIGHT.id().clone(),
        SettingValue::Int(value),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_comes_back_as_it_was() {
        let params = Params::new().with("v", "35");
        assert_eq!(
            back(&params),
            Some(Command::SetSetting(
                FRONTLIGHT.id().clone(),
                SettingValue::Int(35)
            ))
        );
    }

    #[test]
    fn nonsense_is_ignored_or_kept_in_range() {
        assert_eq!(back(&Params::new()), None);
        assert_eq!(back(&Params::new().with("v", "x")), None);
        let wild = back(&Params::new().with("v", "9000")).unwrap();
        assert_eq!(
            wild,
            Command::SetSetting(FRONTLIGHT.id().clone(), SettingValue::Int(100))
        );
    }
}
