//! `command.firmware_dual_boot` (`docs/abi/device.md` 5.3 "Dual boot",
//! `docs/dual-boot.md`) as a job of the firmware queue.
//!
//! 1. **Gate:** an image waiting for its restart is reason 21, a device with
//!    no second app partition to share reason 19, a `paperpaw.ini` that cannot
//!    be read whole (or is over its size) reason 16, and nothing is written.
//!    Where nothing would change (mode 1 and the file says `off` already;
//!    mode 0 and it does not) the job ends with reason 0, asking and writing
//!    nothing.
//! 2. **Ask** (mode 1 only, always, whatever grant): the host's own
//!    confirmation ([`PluginQuestion`] of kind 1, worded by `plugin_confirm`).
//!    No is reason 2, dismissed reason 3, and nothing changes.
//! 3. **Write:** `paperpaw.ini` whole ([`dual_boot_ini::set`]), and only then
//!    (mode 1) the note `.paperpaw/boot.txt` goes: a cut at any point leaves
//!    the setup declared, the safe side. A failed write is reason 16 and no
//!    restart.
//! 4. **After:** a yes to mode 1 restarts the device ([`After::Restart`]);
//!    mode 0 never does, but from the write on the host treats the setup as
//!    declared until the next start ([`After::Declared`]).
//!
//! `step` never allocates beyond the ini's own bounded copy and never panics
//! on card content; `abort` is infallible and the next step ends with it.
use paperpaw_platform::firmware::Text;

use super::dual_boot_ini::{self, Choice, Failure, Mode};
use super::firmware_install::KIND_FIRMWARE;
use super::plugin_job::{
    Answer, Finished, JobEnv, JobStep, PluginJob, PluginQuestion, Relation, Uses, reason,
};

/// The note a dual-boot setup leaves for plugins (`docs/dual-boot.md`).
pub(crate) const NOTE_PATH: &str = ".paperpaw/boot.txt";

/// `install_done.reason` 21: an installed image waits for its restart.
const RESTART_PENDING: u8 = 21;

/// What the host does once the job ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum After {
    #[default]
    Nothing,
    /// The file now says `off` and the user said yes: restart the device.
    Restart,
    /// The file now says `auto`: protect until the next start, no restart.
    Declared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Start,
    Ask,
    Waiting,
    Write,
    Ended,
}

/// One `firmware_dual_boot` request.
pub(crate) struct DualBoot {
    owner: u64,
    request: u32,
    mode: Mode,
    /// The device has a second app partition to share.
    capable: bool,
    /// An installed image waits for its restart (set by the driver).
    waiting: bool,
    phase: Phase,
    answer: Option<Answer>,
    stopped: Option<u8>,
    after: After,
}

impl DualBoot {
    /// The job of `owner`'s `request`; `capable` says whether the device has a
    /// second app partition to share.
    pub(crate) fn new(owner: u64, request: u32, mode: Mode, capable: bool) -> Self {
        Self {
            owner,
            request,
            mode,
            capable,
            waiting: false,
            phase: Phase::Start,
            answer: None,
            stopped: None,
            after: After::Nothing,
        }
    }

    fn end(&mut self, why: u8) -> JobStep {
        self.phase = Phase::Ended;
        JobStep::Done(Finished::refused(why))
    }

    fn start(&mut self, env: &JobEnv<'_>) -> JobStep {
        if self.waiting {
            return self.end(RESTART_PENDING);
        }
        if !self.capable {
            return self.end(reason::NOT_HERE);
        }
        let choice = match dual_boot_ini::card_choice(env.storage) {
            Ok(choice) => choice,
            Err(Failure::Unreadable | Failure::NoMemory | Failure::Write | Failure::Unexpected) => {
                return self.end(reason::CARD);
            }
        };
        let off = choice == Choice::Off;
        match (self.mode, off) {
            // Nothing would change: nothing asked, nothing written.
            (Mode::Off, true) | (Mode::Auto, false) => self.end(reason::OK),
            (Mode::Off, false) => {
                self.phase = Phase::Ask;
                JobStep::More
            }
            (Mode::Auto, true) => {
                self.phase = Phase::Write;
                JobStep::More
            }
        }
    }

    fn write(&mut self, env: &JobEnv<'_>) -> JobStep {
        if dual_boot_ini::set(env.storage, self.mode).is_err() {
            return self.end(reason::CARD);
        }
        if self.mode == Mode::Off {
            // The note goes after the file: a cut leaves the setup declared.
            // A note that stays is harmless: `off` wins over it.
            let _ = env.storage.remove(NOTE_PATH);
        }
        self.after = if self.mode == Mode::Off {
            After::Restart
        } else {
            After::Declared
        };
        self.end(reason::OK)
    }

    fn question(&self) -> PluginQuestion {
        PluginQuestion {
            owner: self.owner,
            request: self.request,
            kind: KIND_FIRMWARE,
            id: Text::new(""),
            name: Text::new(""),
            version: Text::new(""),
            signer: Text::new(""),
            key: None,
            sideload: false,
            relation: Relation::Newer,
            uses: Uses {
                keys: [Text::new(""); super::plugin_job::USES_SHOWN],
                count: 0,
                more: false,
            },
        }
    }
}

impl PluginJob for DualBoot {
    fn owner(&self) -> u64 {
        self.owner
    }

    fn request(&self) -> u32 {
        self.request
    }

    fn kind(&self) -> u32 {
        KIND_FIRMWARE
    }

    fn step(&mut self, env: &JobEnv<'_>) -> JobStep {
        if let Some(why) = self.stopped {
            return self.end(why);
        }
        match self.phase {
            Phase::Start => self.start(env),
            Phase::Ask => {
                self.phase = Phase::Waiting;
                JobStep::Ask(self.question())
            }
            Phase::Waiting => match self.answer {
                None => JobStep::Wait,
                Some(Answer::Yes | Answer::YesAndData) => {
                    self.phase = Phase::Write;
                    JobStep::More
                }
                Some(Answer::No) => self.end(reason::DECLINED),
                Some(Answer::Dismissed) => self.end(reason::DISMISSED),
            },
            Phase::Write => self.write(env),
            Phase::Ended => JobStep::Done(Finished::refused(reason::OTHER)),
        }
    }

    fn answer(&mut self, answer: Answer) {
        if self.phase == Phase::Waiting {
            self.answer = Some(answer);
        }
    }

    fn abort(&mut self, why: u8) {
        self.stopped = Some(why);
    }

    fn image_waiting(&mut self, waiting: bool) {
        self.waiting = waiting;
    }

    fn after(&self) -> After {
        self.after
    }
}

#[cfg(test)]
#[path = "dual_boot_job_tests.rs"]
mod tests;
