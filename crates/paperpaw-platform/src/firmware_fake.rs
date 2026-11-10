//! A [`FirmwareHost`] that does what it is told, for the kernel's tests and
//! the desktop emulator: a scripted check, a write that counts blocks, and a
//! failure that can be put at any call of any stage (a card error, a changed
//! image, a cut mid-write).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::Storage;
use crate::firmware::{
    BUILT_IN_MAX, CheckFacts, FirmwareHost, Reason, Running, Step, Text, WRITE_BLOCK,
};

/// Where a scripted failure strikes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    BeginCheck,
    Check,
    BeginWrite,
    Write,
    Readback,
    Commit,
    Restart,
}

/// What the fake was asked, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    BeginCheck {
        path: String,
    },
    CheckStep,
    BeginWrite {
        path: String,
        digest: [u8; 32],
        key: Option<[u8; 32]>,
        foreign: bool,
    },
    WriteStep,
    ReadbackStep,
    Commit,
    Abandon,
    Restart,
    RestartRunning,
}

/// What is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Nothing,
    Check { steps: u32 },
    Write { done: u32 },
    Readback { steps: u32 },
    Written,
}

/// The scripted firmware.
#[derive(Debug)]
pub struct FakeFirmware {
    version: String,
    project: String,
    /// What the running firmware reports.
    pub chip: u16,
    pub room: u64,
    pub undone: bool,
    /// What a check finds.
    pub facts: CheckFacts,
    /// The plugins the checked image has built in, in its order; ignored
    /// for a foreign check.
    pub built_ins: Vec<String>,
    /// Calls of `check_step` before it is done (each is a `Progress`).
    pub check_steps: u32,
    /// Calls of `readback_step` before it is done.
    pub readback_steps: u32,
    /// Where a call fails, and with what: the `nth` (from 0) call of `stage`.
    pub fail: Option<(Stage, u32, Reason)>,
    /// Whether `restart` works.
    pub can_restart: bool,
    /// Every call, in order.
    pub log: Vec<Call>,
    /// Counts of each stage's calls, for `fail`.
    counts: [u32; 7],
    open: Open,
    /// Set by a `commit` that worked.
    pub committed: bool,
    /// What `dual_boot_capable` and `dual_boot_declared` say, and whether
    /// `restart_running` works.
    pub dual_capable: bool,
    pub dual_declared: bool,
    pub can_restart_running: bool,
}

impl FakeFirmware {
    /// A fake running `version` of `project` on `chip` with room for an
    /// image, that finds `facts` in every image it checks.
    pub fn new(version: &str, project: &str, chip: u16, facts: CheckFacts) -> Self {
        Self {
            version: String::from(version),
            project: String::from(project),
            chip,
            room: 6 * 1024 * 1024,
            undone: false,
            facts,
            built_ins: Vec::new(),
            check_steps: 1,
            readback_steps: 1,
            fail: None,
            can_restart: true,
            log: Vec::new(),
            counts: [0; 7],
            open: Open::Nothing,
            committed: false,
            dual_capable: false,
            dual_declared: false,
            can_restart_running: false,
        }
    }

    /// Whether a check or a write is open.
    pub fn busy(&self) -> bool {
        !matches!(self.open, Open::Nothing | Open::Written)
    }

    /// Whether an image was written and awaits its commit.
    pub fn written(&self) -> bool {
        self.open == Open::Written
    }

    /// The reason scripted for this call of `stage`, counting it.
    fn strike(&mut self, stage: Stage) -> Option<Reason> {
        let n = self.counts[stage as usize];
        self.counts[stage as usize] += 1;
        match self.fail {
            Some((at, nth, reason)) if at == stage && nth == n => Some(reason),
            _ => None,
        }
    }

    /// A failed step ends the job it was in.
    fn failed<T>(&mut self, reason: Reason) -> Step<T> {
        self.open = Open::Nothing;
        Step::Failed(reason)
    }
}

impl FirmwareHost for FakeFirmware {
    fn running(&self) -> Running<'_> {
        Running::new(
            &self.version,
            &self.project,
            self.chip,
            self.room,
            self.undone,
        )
    }

    fn begin_check(&mut self, path: &str) -> Result<(), Reason> {
        self.log.push(Call::BeginCheck {
            path: String::from(path),
        });
        if let Some(reason) = self.strike(Stage::BeginCheck) {
            return Err(reason);
        }
        if self.busy() {
            return Err(Reason::OTHER);
        }
        self.open = Open::Check { steps: 0 };
        Ok(())
    }

    fn check_step(&mut self, _: &dyn Storage, built_in: &mut dyn FnMut(&str)) -> Step<CheckFacts> {
        self.log.push(Call::CheckStep);
        let Open::Check { steps } = self.open else {
            return Step::Failed(Reason::OTHER);
        };
        if let Some(reason) = self.strike(Stage::Check) {
            return self.failed(reason);
        }
        if steps == 0 && !self.facts.foreign {
            for id in &self.built_ins {
                let mut end = id.len().min(BUILT_IN_MAX);
                while !id.is_char_boundary(end) {
                    end -= 1;
                }
                built_in(&id[..end]);
            }
        }
        if steps < self.check_steps {
            self.open = Open::Check { steps: steps + 1 };
            return Step::Progress {
                done: steps + 1,
                total: self.check_steps + 1,
            };
        }
        self.open = Open::Nothing;
        Step::Done(self.facts)
    }

    fn begin_write(
        &mut self,
        path: &str,
        digest: [u8; 32],
        key: Option<[u8; 32]>,
        foreign: bool,
    ) -> Result<(), Reason> {
        self.log.push(Call::BeginWrite {
            path: String::from(path),
            digest,
            key,
            foreign,
        });
        if let Some(reason) = self.strike(Stage::BeginWrite) {
            return Err(reason);
        }
        if self.room == 0 {
            return Err(Reason::NOT_HERE);
        }
        if self.busy() {
            return Err(Reason::OTHER);
        }
        // An install waiting for its restart is given up.
        self.committed = false;
        self.open = Open::Write { done: 0 };
        Ok(())
    }

    fn write_step(&mut self, _: &dyn Storage) -> Step<()> {
        self.log.push(Call::WriteStep);
        let Open::Write { done } = self.open else {
            return Step::Failed(Reason::OTHER);
        };
        if let Some(reason) = self.strike(Stage::Write) {
            return self.failed(reason);
        }
        let total = self.facts.size;
        let done = done.saturating_add(WRITE_BLOCK).min(total);
        if done < total {
            self.open = Open::Write { done };
            return Step::Progress { done, total };
        }
        self.open = Open::Readback { steps: 0 };
        Step::Done(())
    }

    fn readback_step(&mut self) -> Step<()> {
        self.log.push(Call::ReadbackStep);
        let Open::Readback { steps } = self.open else {
            return Step::Failed(Reason::OTHER);
        };
        if let Some(reason) = self.strike(Stage::Readback) {
            return self.failed(reason);
        }
        if steps < self.readback_steps {
            self.open = Open::Readback { steps: steps + 1 };
            return Step::Progress {
                done: steps + 1,
                total: self.readback_steps + 1,
            };
        }
        self.open = Open::Written;
        Step::Done(())
    }

    fn commit(&mut self) -> Result<(), Reason> {
        self.log.push(Call::Commit);
        // Ok or Err, the job is over.
        let was = core::mem::replace(&mut self.open, Open::Nothing);
        if let Some(reason) = self.strike(Stage::Commit) {
            return Err(reason);
        }
        if was != Open::Written {
            return Err(Reason::OTHER);
        }
        self.committed = true;
        Ok(())
    }

    /// A no-op where nothing is open.
    fn abandon(&mut self) {
        self.log.push(Call::Abandon);
        self.open = Open::Nothing;
    }

    fn restart(&mut self) -> Result<(), Reason> {
        self.log.push(Call::Restart);
        if let Some(reason) = self.strike(Stage::Restart) {
            return Err(reason);
        }
        if self.can_restart && self.committed {
            Ok(())
        } else {
            Err(Reason::NOT_HERE)
        }
    }

    fn dual_boot_capable(&self) -> bool {
        self.dual_capable
    }

    fn dual_boot_declared(&self) -> bool {
        self.dual_declared
    }

    fn restart_running(&mut self) -> Result<(), Reason> {
        self.log.push(Call::RestartRunning);
        if self.can_restart_running {
            Ok(())
        } else {
            Err(Reason::NOT_HERE)
        }
    }
}

/// A fake's check facts for a signed PaperPaw update of `size` bytes.
pub fn paperpaw_facts(version: &str, project: &str, size: u32) -> CheckFacts {
    CheckFacts::new(
        true,
        size,
        Text::new(version),
        Text::new(project),
        crate::firmware::Signer::PaperPaw {
            name: Text::new("PaperPaw"),
            key: [7; 32],
        },
        false,
        [9; 32],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoStorage;

    fn fake() -> FakeFirmware {
        let mut fake = FakeFirmware::new(
            "0.5.0",
            "paperpaw-x4",
            5,
            paperpaw_facts("0.6.0", "paperpaw-x4", 3 * WRITE_BLOCK),
        );
        fake.built_ins = alloc::vec![String::from("stats"), String::from("weather")];
        fake
    }

    /// A whole install: check, write block by block, read back, commit,
    /// restart; the built-ins stream once.
    #[test]
    fn a_whole_install_goes_through_every_stage() {
        let mut fake = fake();
        let storage = NoStorage;
        fake.begin_check("update.ppu").unwrap();
        let mut seen: Vec<String> = Vec::new();
        let facts = loop {
            match fake.check_step(&storage, &mut |id| seen.push(String::from(id))) {
                Step::Progress { .. } => {}
                Step::Done(facts) => break facts,
                Step::Failed(r) => panic!("{r:?}"),
            }
        };
        assert_eq!(seen, ["stats", "weather"]);
        fake.begin_write("update.ppu", facts.sha256, None, false)
            .unwrap();
        let mut writes = 0;
        while !matches!(fake.write_step(&storage), Step::Done(())) {
            writes += 1;
        }
        assert_eq!(writes, 2, "three blocks: two Progress, then Done");
        while !matches!(fake.readback_step(), Step::Done(())) {}
        assert!(fake.written());
        fake.commit().unwrap();
        assert!(fake.committed);
        fake.restart().unwrap();
    }

    /// A failure at any call of any stage ends the job and says why; a cut
    /// mid-write leaves nothing committed.
    #[test]
    fn a_scripted_failure_ends_the_job_it_strikes() {
        let storage = NoStorage;
        let mut fake = fake();
        fake.fail = Some((Stage::Write, 1, Reason::CARD));
        fake.begin_write("u.ppu", [0; 32], None, false).unwrap();
        assert!(matches!(fake.write_step(&storage), Step::Progress { .. }));
        assert_eq!(fake.write_step(&storage), Step::Failed(Reason::CARD));
        assert!(!fake.busy() && !fake.committed);
        // The next job begins clean.
        fake.begin_write("u.ppu", [0; 32], None, false).unwrap();

        let mut fake = self::fake();
        fake.fail = Some((Stage::Readback, 0, Reason::CHANGED));
        fake.begin_write("u.ppu", [0; 32], None, false).unwrap();
        while !matches!(fake.write_step(&storage), Step::Done(())) {}
        assert_eq!(fake.readback_step(), Step::Failed(Reason::CHANGED));
        assert!(fake.commit().is_err());

        let mut fake = self::fake();
        fake.fail = Some((Stage::BeginCheck, 0, Reason::OTHER));
        assert_eq!(fake.begin_check("u.ppu"), Err(Reason::OTHER));
    }

    /// One job at a time; nothing is installed where there is no room.
    #[test]
    fn a_second_begin_is_a_failure_and_no_room_is_not_here() {
        let mut fake = fake();
        fake.begin_check("a").unwrap();
        assert_eq!(fake.begin_check("b"), Err(Reason::OTHER));
        assert_eq!(
            fake.begin_write("b", [0; 32], None, false),
            Err(Reason::OTHER)
        );
        fake.abandon();
        fake.begin_check("b").unwrap();
        fake.abandon();
        fake.room = 0;
        assert_eq!(
            fake.begin_write("c", [0; 32], None, false),
            Err(Reason::NOT_HERE)
        );
        // A check still works.
        fake.begin_check("c").unwrap();
        assert_eq!(fake.restart(), Err(Reason::NOT_HERE));
    }

    /// Another firmware's image has no list of built-ins to stream.
    #[test]
    fn a_foreign_check_streams_no_built_ins() {
        let mut fake = fake();
        let mut facts = paperpaw_facts("1.0", "crosspoint", 100);
        facts.foreign = true;
        fake.facts = facts;
        fake.begin_check("x.bin").unwrap();
        let mut seen = 0;
        while !matches!(
            fake.check_step(&NoStorage, &mut |_| seen += 1),
            Step::Done(_)
        ) {}
        assert_eq!(seen, 0);
    }
}
