//! `remove_plugin` (`docs/abi/device.md` 5.2, 5.4; `files.md` 4, decision 124):
//! the removal of an installed plugin as a job of the queue, and the same steps
//! at the next start after a cut.
//!
//! 1. **Probe:** `core` and the plugins built into the image are taken (reason
//!    15); a folder `plugins/<id>` must be there (else reason 1).
//! 2. **Ask:** one host confirmation with three answers: remove and keep the
//!    data, remove with the data, cancel. The trusted-installer grant never
//!    skips it (decision 124). Declined is reason 2, dismissed 3, and nothing
//!    changes.
//! 3. **Journal** (`plugins/.journal`, record kind 1, written like an install's),
//!    from which the removal only goes *forward* (the user confirmed it).
//! 4. **Close:** the plugin's instances are closed by the host (`JobStep::Unload`).
//! 5. **Move:** `plugins/<id>` to `plugins/.removing-<id>` and, with the data,
//!    `.paperpaw/plugins/<id>` to `.paperpaw/plugins/.removing-<id>`. Both paths
//!    are built from the validated id alone, compared whole (removing `battery`
//!    leaves `battery-plus`), never from anything a plugin gave. Every move is
//!    the card's journalled `rename`, so a cut leaves a folder in one place or
//!    the other.
//! 6. **Forget:** with the data, the host forgets what it holds of the plugin
//!    (`JobStep::Forget`): its secrets (`core.secrets`, if wired) and its
//!    grants (`core.grant.<kind>.<id>`, so a reinstall of the id does not
//!    regain a trusted installer). Safe to repeat. It comes **after the
//!    moves**, so a removal that fails with nothing moved loses neither, and
//!    **before the journal goes**, so a cut keeps the journal: the start's
//!    replay has no host yet and stops here (`Replayed::Forgetting`), and the
//!    kernel's first pass finishes the journal with the host
//!    (`plugin_swap::resume`). Nothing is forgotten without the user's yes
//!    with the data.
//!    A host that fails at [`plugin_swap::FORGET_TRIES`] starts in a row does not
//!    keep the journal (and so every later install) waiting for ever: the removal
//!    is finished without forgetting, the secrets stay on the card, it is logged
//!    and the user is told (`core.plugin.unforgotten`).
//! 7. **Journal gone, then clean:** the journal is removed once the plugin is
//!    out of its place and its secrets are forgotten; the moved folders
//!    (`.removing-<id>`) are then removed **a bounded piece a step**
//!    (`Storage::remove_tree_step`), so a large data folder never holds the
//!    device for long. A cut or a failure there leaves the folders to the start's
//!    sweep, which goes on in the same bounded way (`plugin_start::sweep`); no
//!    journal is needed for garbage no plugin can reach.
//!
//! [`Removing`] is the one state machine for the live job and for the start's
//! replay: each step looks at the card and does the next thing, so a cut at any
//! point is finished by the same code. A state that no step of it makes (the
//! target and its moved copy at once) can never be finished: the start sets the
//! journal aside (`plugin_swap::replay`) instead of blocking every later change.
//!
//! The result of closing the plugin's instances (`JobStep::Unload`) is not
//! read: an instance that does not close ends with the device's next start, and
//! the removal goes on (the plugin is out of the list before its folder moves).
//!
//! Pins and origin marks of a removed plugin stay (a reinstall of the id must
//! still come from its pinned author). A loaded code plugin's flash room
//! returns at the next start (`install_done.flags` bit 1).
use alloc::{boxed::Box, string::String};
use paperpaw_platform::{Storage, StorageError, firmware::Text};

use super::firmware_install::BuiltIns;
use super::plugin_install::installed_of;
use super::plugin_job::{
    Answer, DONE_AT_NEXT_START, DONE_DATA_KEPT, Finished, ID_MAX, JobEnv, JobStep, KIND_REMOVAL,
    PluginJob, PluginQuestion, Relation, USE_KEY_MAX, USES_SHOWN, Uses, io_buffer, reason,
};
use super::plugin_swap::{
    JOURNAL, Removal, begin_removal, data_path, data_removing_path, io, present, removing_path,
    target_path,
};
use super::shared::try_box;

/// What a step of the removal came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoveStep {
    More,
    /// The host must forget the plugin's secrets and grants; the answer is
    /// [`Removing::forgotten`].
    Forget,
    /// The plugin is gone and the journal removed.
    Done,
    /// Stopped with this reason; the journal stays (the next start finishes).
    Failed(u8),
    /// A state no step of this removal makes: it can never be finished.
    Stuck,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Code,
    Data,
    Forget,
    Journal,
    CleanCode,
    CleanData,
    Done,
}

/// The removal of one plugin, one card action a step.
pub(crate) struct Removing {
    removal: Removal,
    code: String,
    code_out: String,
    data: String,
    data_out: String,
    part: Part,
    /// Where the bounded removal of the folder being cleaned goes on.
    place: u64,
    /// The host could not forget what it holds of the plugin.
    forget_failed: bool,
    /// A folder was moved already: the removal can only go forward.
    moved: bool,
}

impl Removing {
    /// The removal of `removal`, its journal written or read. `None` for an id
    /// no plugin has, or no memory.
    pub(crate) fn new(removal: Removal) -> Option<Self> {
        let id = removal.id.as_str();
        Some(Self {
            code: target_path(id)?,
            code_out: removing_path(id)?,
            data: data_path(id)?,
            data_out: data_removing_path(id)?,
            removal,
            part: Part::Code,
            place: 0,
            forget_failed: false,
            moved: false,
        })
    }

    /// Whether a folder was moved: from then on a failure keeps the journal.
    pub(crate) fn moved(&self) -> bool {
        self.moved
    }

    /// Whether the journal is gone: what is left is garbage the sweep finds.
    pub(crate) fn settled(&self) -> bool {
        matches!(self.part, Part::CleanCode | Part::CleanData | Part::Done)
    }

    /// The host's answer to [`RemoveStep::Forget`]; `false`: it could not.
    pub(crate) fn forgotten(&mut self, ok: bool) {
        if self.part == Part::Forget {
            if ok {
                self.part = Part::Journal;
            } else {
                self.forget_failed = true;
            }
        }
    }

    /// One step.
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> RemoveStep {
        match self.part {
            Part::Code => {
                let (from, to) = (self.code.clone(), self.code_out.clone());
                self.relocate(storage, &from, &to, Part::Data)
            }
            Part::Data => {
                if !self.removal.data {
                    self.part = Part::Journal;
                    return RemoveStep::More;
                }
                let (from, to) = (self.data.clone(), self.data_out.clone());
                self.relocate(storage, &from, &to, Part::Forget)
            }
            Part::Forget if self.forget_failed => RemoveStep::Failed(reason::CARD),
            Part::Forget => RemoveStep::Forget,
            Part::Journal => match storage.remove(JOURNAL) {
                Ok(()) | Err(StorageError::NotFound) => {
                    self.part = Part::CleanCode;
                    RemoveStep::More
                }
                Err(error) => RemoveStep::Failed(io(error)),
            },
            Part::CleanCode => {
                let gone = self.code_out.clone();
                self.clean(storage, &gone, Part::CleanData)
            }
            Part::CleanData => {
                if !self.removal.data {
                    self.part = Part::Done;
                    return RemoveStep::Done;
                }
                let gone = self.data_out.clone();
                self.clean(storage, &gone, Part::Done)
            }
            Part::Done => RemoveStep::Done,
        }
    }

    /// Moves `from` to `to` unless it is moved already.
    fn relocate(&mut self, storage: &dyn Storage, from: &str, to: &str, next: Part) -> RemoveStep {
        let (Ok(here), Ok(there)) = (present(storage, from), present(storage, to)) else {
            return RemoveStep::Failed(reason::CARD);
        };
        match (here, there) {
            (true, false) => {
                if let Err(error) = storage.rename(from, to) {
                    return RemoveStep::Failed(io(error));
                }
                self.moved = true;
            }
            // Moved already (a cut after the move), or never there.
            (false, _) => {}
            // The folder and its moved copy at once: no step of this removal
            // makes it, and none can tell which is the plugin's.
            (true, true) => return RemoveStep::Stuck,
        }
        self.part = next;
        RemoveStep::More
    }

    /// Removes what was moved aside, where it is, a bounded piece a step
    /// (`Storage::remove_tree_step`). The journal is gone: an error leaves the
    /// rest to the start's sweep.
    fn clean(&mut self, storage: &dyn Storage, gone: &str, next: Part) -> RemoveStep {
        match storage.remove_tree_step(gone, false, self.place) {
            Ok(step) if !step.done => {
                self.place = step.next;
                return RemoveStep::More;
            }
            Ok(_) | Err(_) => {}
        }
        self.place = 0;
        self.part = next;
        if next == Part::Done {
            return RemoveStep::Done;
        }
        RemoveStep::More
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Start,
    Ask,
    Waiting,
    Journal,
    Unload,
    Unloading,
    Removing,
    /// Waiting for the host to forget the plugin's secrets and grants.
    Forgetting,
    Ended,
}

/// A removal in the queue.
pub(crate) struct PluginRemove {
    owner: u64,
    request: u32,
    id: Text<ID_MAX>,
    phase: Phase,
    question: Option<Box<PluginQuestion>>,
    answer: Option<Answer>,
    /// The user chose to remove the data as well.
    data: bool,
    /// A code plugin: its flash room returns at the next start.
    code: bool,
    removing: Option<Box<Removing>>,
    stopped: Option<u8>,
    /// The host stopped it after the journal: the plugin's instances are not
    /// waited for (the device is going down), the removal still ends.
    aborted: bool,
}

impl PluginRemove {
    /// The removal of the installed plugin `id`.
    ///
    /// # Errors
    /// `None` for no memory.
    pub(crate) fn new(owner: u64, request: u32, id: &str) -> Option<Self> {
        Some(Self {
            owner,
            request,
            id: Text::new(id),
            phase: Phase::Start,
            question: None,
            answer: None,
            data: false,
            code: false,
            removing: None,
            stopped: None,
            aborted: false,
        })
    }

    fn end(&mut self, why: u8) -> JobStep {
        self.phase = Phase::Ended;
        self.question = None;
        self.removing = None;
        JobStep::Done(Finished::refused(why))
    }

    #[inline(never)]
    fn start(&mut self, env: &JobEnv<'_>) -> JobStep {
        let id = self.id.as_str();
        let (Some(target), Some(_)) = (target_path(id), data_path(id)) else {
            return self.end(reason::ID_TAKEN);
        };
        if env.built_ins.index_of(id).is_some() {
            return self.end(reason::ID_TAKEN);
        }
        match present(env.storage, &target) {
            Ok(true) => {}
            Ok(false) => return self.end(reason::OTHER),
            Err(why) => return self.end(why),
        }
        // A folder an earlier removal moved aside and is still cleaning (cut,
        // or not done yet) would meet this one's move: refused here, before
        // any journal is written, so no later install is blocked by it; the
        // start's sweep finishes it.
        for out in [removing_path(id), data_removing_path(id)] {
            let Some(out) = out else {
                return self.end(reason::OTHER);
            };
            match present(env.storage, &out) {
                Ok(false) => {}
                Ok(true) => return self.end(reason::OTHER),
                Err(why) => return self.end(why),
            }
        }
        let Some(mut buf) = io_buffer() else {
            return self.end(reason::OTHER);
        };
        // A plugin whose manifest cannot be read can still be removed: it is
        // named by its id.
        let (name, version) =
            match super::plugin_folder::read_manifest(env.storage, &target, id, &mut buf) {
                Ok(manifest) => (manifest.name, manifest.version),
                Err(_) => (Text::new(id), Text::new("")),
            };
        self.code = installed_of(env.storage, &target, id, &mut buf).code;
        self.question = try_box(PluginQuestion {
            owner: self.owner,
            request: self.request,
            kind: KIND_REMOVAL,
            id: self.id,
            name,
            version,
            signer: Text::new(""),
            key: None,
            sideload: false,
            relation: Relation::Newer,
            uses: Uses {
                keys: [Text::<USE_KEY_MAX>::new(""); USES_SHOWN],
                count: 0,
                more: false,
            },
        });
        if self.question.is_none() {
            return self.end(reason::OTHER);
        }
        self.phase = Phase::Ask;
        JobStep::More
    }

    #[inline(never)]
    fn journal(&mut self, storage: &dyn Storage, built_ins: &dyn BuiltIns) -> JobStep {
        // Before the removal commits: a device that kept no record of the
        // store's installs takes its plugins for the store's, once.
        if let Some(found) = built_ins.found() {
            super::store_record::adopt(storage, found);
        }
        let removal = Removal {
            id: self.id,
            data: self.data,
        };
        // Built before the journal: a refusal for memory leaves none.
        let Some(removing) = Removing::new(removal).and_then(try_box) else {
            return self.end(reason::OTHER);
        };
        if let Err(why) = begin_removal(storage, &removal) {
            return self.end(why);
        }
        self.removing = Some(removing);
        self.phase = Phase::Unload;
        JobStep::More
    }

    #[inline(never)]
    fn removing(&mut self, storage: &dyn Storage) -> JobStep {
        let Some(removing) = self.removing.as_mut() else {
            return self.end(reason::OTHER);
        };
        match removing.step(storage) {
            RemoveStep::More => JobStep::More,
            RemoveStep::Forget => {
                self.phase = Phase::Forgetting;
                JobStep::Forget(self.id)
            }
            RemoveStep::Done => {
                let mut flags = 0;
                if !self.data {
                    flags |= DONE_DATA_KEPT;
                }
                if self.code {
                    flags |= DONE_AT_NEXT_START;
                }
                self.phase = Phase::Ended;
                JobStep::Done(Finished {
                    reason: reason::OK,
                    facts: None,
                    flags,
                    changed: Some(self.id),
                    needs_draft: 0,
                })
            }
            RemoveStep::Failed(why) => {
                // Nothing moved yet: nothing changed, the journal goes. After
                // a move the journal stays and the next start finishes it.
                if !removing.moved() {
                    let _ = storage.remove(JOURNAL);
                }
                self.end(why)
            }
            // The plugin folder and its moved copy at once: left to the start,
            // which sets the journal aside.
            RemoveStep::Stuck => self.end(reason::OTHER),
        }
    }
}

impl PluginJob for PluginRemove {
    fn owner(&self) -> u64 {
        self.owner
    }

    fn request(&self) -> u32 {
        self.request
    }

    fn kind(&self) -> u32 {
        KIND_REMOVAL
    }

    #[inline(never)]
    fn step(&mut self, env: &JobEnv<'_>) -> JobStep {
        if let Some(why) = self.stopped {
            return self.end(why);
        }
        let storage = env.storage;
        match self.phase {
            Phase::Start => self.start(env),
            Phase::Ask => match self.question.take() {
                Some(question) => {
                    self.phase = Phase::Waiting;
                    JobStep::Ask(*question)
                }
                None => self.end(reason::OTHER),
            },
            Phase::Waiting => match self.answer {
                None => JobStep::Wait,
                Some(Answer::Yes) => {
                    self.data = false;
                    self.phase = Phase::Journal;
                    JobStep::More
                }
                Some(Answer::YesAndData) => {
                    self.data = true;
                    self.phase = Phase::Journal;
                    JobStep::More
                }
                Some(Answer::No) => self.end(reason::DECLINED),
                Some(Answer::Dismissed) => self.end(reason::DISMISSED),
            },
            Phase::Journal => self.journal(storage, env.built_ins),
            Phase::Unload => {
                if self.aborted {
                    self.phase = Phase::Removing;
                    return JobStep::More;
                }
                self.phase = Phase::Unloading;
                JobStep::Unload(self.id)
            }
            Phase::Unloading => {
                if self.aborted {
                    self.phase = Phase::Removing;
                    return JobStep::More;
                }
                JobStep::Wait
            }
            Phase::Removing => self.removing(storage),
            Phase::Forgetting => {
                if self.aborted {
                    // The device is going down: the journal stays, and the
                    // kernel's first pass after the next start finishes it.
                    return self.end(reason::OTHER);
                }
                JobStep::Wait
            }
            Phase::Ended => JobStep::Done(Finished::refused(reason::OTHER)),
        }
    }

    fn answer(&mut self, answer: Answer) {
        if self.phase == Phase::Waiting {
            self.answer = Some(answer);
        }
    }

    fn unloaded(&mut self) {
        if self.phase == Phase::Unloading {
            self.phase = Phase::Removing;
        }
    }

    fn forgotten(&mut self, ok: bool) {
        if self.phase == Phase::Forgetting {
            if let Some(removing) = self.removing.as_mut() {
                removing.forgotten(ok);
            }
            self.phase = Phase::Removing;
        }
    }

    fn must_finish(&self) -> bool {
        // From the journal on the user's yes is carried out, whatever stops.
        matches!(
            self.phase,
            Phase::Unload | Phase::Unloading | Phase::Removing | Phase::Forgetting
        )
    }

    fn keep_without_sender(&self) -> bool {
        matches!(
            self.phase,
            Phase::Journal | Phase::Unload | Phase::Unloading | Phase::Removing | Phase::Forgetting
        ) || (self.phase == Phase::Waiting
            && matches!(self.answer, Some(Answer::Yes | Answer::YesAndData)))
    }

    fn abort(&mut self, why: u8) {
        if self.must_finish() {
            self.aborted = true;
            return;
        }
        self.stopped = Some(why);
        self.question = None;
    }
}

#[cfg(test)]
#[path = "plugin_remove_tests.rs"]
pub(in crate::native_host) mod tests;
