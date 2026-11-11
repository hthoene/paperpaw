//! Plugin requests of native plugins (`docs/abi/registry.md` section 4.2 and
//! 5): `command.request_plugins` and `event.plugins_requested` (77), the
//! answers of the installers (`command.answer_plugin`) and the event
//! `event.plugin_answered` (78), told to the requester only.
//!
//! A request never installs anything: it asks the plugins that declared
//! `uses.install` (and subscribed to 77), which ask the user. It is native
//! only: a native plugin asks native installers; interface-6 plugins keep
//! their own bus path until activation.
//!
//! * **Lazy.** The table of open requests is an empty `Vec` until the first
//!   request: a device that never asks costs nothing.
//! * **Bounded.** At most [`MAX_OPEN`] requests are open device-wide (`ERR_FULL`
//!   at submit beyond), each at most 16 ids of at most 64 bytes; every copy is
//!   a fallible allocation.
//! * **Ids already on the device are dropped**; with none left nothing is
//!   sent. With no installer able to answer, every id is answered `failed` at
//!   once (request number 0: such a request is kept nowhere).
//! * **Edges.** 77 and 78 are held encoded in the plugins' queues
//!   (`core_events`, `EDGE_TAGS`), never coalesced; one that does not fit is
//!   dropped and logged. A request whose 77 reached nobody is answered
//!   `failed` at once and not kept.
//! * **Answers** (`commands_answers.rs`) are checked at submit against what
//!   the table held then ([`AnswerView`]) and applied after the callback
//!   ([`Kernel::native_apply_answers`]): the first answer for an id wins, the
//!   host checks itself that a plugin said to be installed is on the device,
//!   and a request is closed when every id is answered, when no installer is
//!   left to answer, or when its requester is gone
//!   ([`Kernel::native_requests_settle`]).
use super::registrations as wire;
use crate::Kernel;
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_NO_MEMORY};
use paperpaw_sdk::Id;

/// The most requests open device-wide.
pub(crate) const MAX_OPEN: usize = 4;
/// `event.plugin_answered.outcome`: the plugin was installed.
pub(crate) const OUTCOME_INSTALLED: u32 = 0;
/// `event.plugin_answered.outcome`: the plugin could not be had.
pub(crate) const OUTCOME_FAILED: u32 = 2;
/// The last outcome the host knows (3, not found); any other is `ERR_INVALID`.
pub(crate) const OUTCOME_LAST: u32 = 3;
/// How many open ids a stage can tell apart (see [`AnswerView`]).
const VIEW_IDS: usize = 16;

/// One id of a request and whether it was answered.
struct Asked {
    plugin: Box<str>,
    answered: bool,
}

/// An open request.
pub(crate) struct Request {
    number: u32,
    by: Box<str>,
    ids: Vec<Asked>,
}

impl Request {
    /// The requester.
    pub(crate) fn by(&self) -> &str {
        &self.by
    }

    /// The request's number.
    pub(crate) fn number(&self) -> u32 {
        self.number
    }

    /// The ids not answered yet.
    pub(crate) fn open_ids(&self) -> impl Iterator<Item = &str> {
        self.ids
            .iter()
            .filter(|asked| !asked.answered)
            .map(|asked| &*asked.plugin)
    }
}

/// What a command stage knows of the requests at the start of a callback: the
/// submitter's right to answer and which (request, id) pairs were open, as
/// 32-bit marks (no allocation, a few bytes in [`StageInputs`]). More open
/// ids than [`VIEW_IDS`] set `more`: an answer then passes the stage unless
/// it is a repeat, and the apply checks it exactly (a stale one is dropped).
///
/// [`StageInputs`]: super::screen_stage::StageInputs
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AnswerView {
    pub(crate) installer: bool,
    more: bool,
    count: u8,
    marks: [u32; VIEW_IDS],
}

/// The mark of an id of a request (FNV-1a over the number and the id).
fn mark(request: u32, plugin: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in request.to_le_bytes().iter().chain(plugin.as_bytes()) {
        hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

impl AnswerView {
    /// Whether the id of the request was open (it may have been, if the table
    /// held more than the view can tell).
    pub(crate) fn was_open(&self, request: u32, plugin: &str) -> bool {
        self.more
            || self.marks[..usize::from(self.count)]
                .iter()
                .any(|held| *held == mark(request, plugin))
    }

    fn push(&mut self, request: u32, plugin: &str) {
        match self.marks.get_mut(usize::from(self.count)) {
            Some(slot) => {
                *slot = mark(request, plugin);
                self.count += 1;
            }
            None => self.more = true,
        }
    }
}

/// An answer the stage accepted, for the kernel to apply after the callback.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) request: u32,
    pub(crate) plugin: Box<str>,
    pub(crate) outcome: u32,
}

/// The open requests.
#[derive(Default)]
pub(crate) struct PluginRequests {
    open: Vec<Request>,
    last: u32,
}

/// `text` copied with the allocation checked.
pub(crate) fn boxed(text: &str) -> Result<Box<str>, i32> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(text);
    Ok(copy.into_boxed_str())
}

impl PluginRequests {
    /// How many requests are open.
    pub(crate) fn open_count(&self) -> usize {
        self.open.len()
    }

    /// The next request number (never 0, none that is open).
    fn number(&mut self) -> u32 {
        loop {
            self.last = self.last.wrapping_add(1).max(1);
            if !self.open.iter().any(|request| request.number == self.last) {
                return self.last;
            }
        }
    }

    /// A number for a request that is answered at once and kept nowhere.
    pub(crate) fn number_unkept(&mut self) -> u32 {
        self.number()
    }

    /// Opens a request of `by` for `plugins`.
    ///
    /// # Errors
    /// `ERR_FULL` with [`MAX_OPEN`] open, `ERR_NO_MEMORY`.
    pub(crate) fn begin(&mut self, by: &str, plugins: &[Id]) -> Result<u32, i32> {
        if self.open.len() >= MAX_OPEN {
            return Err(ERR_FULL);
        }
        let mut ids = Vec::new();
        ids.try_reserve_exact(plugins.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        for plugin in plugins {
            ids.push(Asked {
                plugin: boxed(plugin.as_str())?,
                answered: false,
            });
        }
        let by = boxed(by)?;
        self.open.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let number = self.number();
        self.open.push(Request { number, by, ids });
        Ok(number)
    }

    /// Closes request `number` (nothing was asked of anyone).
    pub(crate) fn discard(&mut self, number: u32) {
        self.open.retain(|request| request.number != number);
    }

    /// Records the answer for `plugin` of request `number`: the first answer
    /// for an id wins. The request is closed when its last id was answered.
    /// Gives the requester, or `None` for a request or id not open.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (nothing was recorded).
    pub(crate) fn answer(&mut self, number: u32, plugin: &str) -> Result<Option<Box<str>>, i32> {
        let Some(at) = self
            .open
            .iter()
            .position(|request| request.number == number)
        else {
            return Ok(None);
        };
        let request = &mut self.open[at];
        let Some(asked) = request
            .ids
            .iter_mut()
            .find(|asked| !asked.answered && &*asked.plugin == plugin)
        else {
            return Ok(None);
        };
        let by = boxed(&request.by)?;
        asked.answered = true;
        if request.ids.iter().all(|asked| asked.answered) {
            self.open.remove(at);
        }
        Ok(Some(by))
    }

    /// Takes out the first request `done` says is finished.
    pub(crate) fn take_finished(&mut self, done: impl Fn(&Request) -> bool) -> Option<Request> {
        let at = self.open.iter().position(done)?;
        Some(self.open.remove(at))
    }

    /// What a stage may know of the open ids (see [`AnswerView`]).
    pub(crate) fn view(&self, installer: bool) -> AnswerView {
        let mut view = AnswerView {
            installer,
            ..AnswerView::default()
        };
        for request in &self.open {
            for plugin in request.open_ids() {
                view.push(request.number, plugin);
            }
        }
        view
    }
}

impl Kernel {
    /// Whether `id` is on the device (any version, on or off), or is `core`.
    fn plugin_on_device(&self, id: &str) -> bool {
        self.shared.plugins.get(id).is_some()
            || self.is_native_plugin(id)
            || id == crate::core_plugin::CORE.as_str()
    }

    /// The ids of `plugins` not on the device, each once.
    fn wanted_plugins(&self, plugins: Vec<Id>) -> Vec<Id> {
        let mut wanted: Vec<Id> = Vec::new();
        for id in plugins {
            if !self.plugin_on_device(id.as_str()) && !wanted.contains(&id) {
                wanted.push(id);
            }
        }
        wanted
    }

    /// How many plugin requests are open device-wide.
    pub(crate) fn native_requests_open(&self) -> usize {
        self.shared
            .native_instances
            .plugin_requests
            .try_borrow()
            .map_or(0, |table| table.open_count())
    }

    /// What a stage of `owner_key` may know of the requests
    /// (see [`AnswerView`]); nothing is allocated, and nothing is read for a
    /// plugin that may not answer.
    pub(crate) fn native_answer_view(&self, owner_key: u64) -> AnswerView {
        let instances = &self.shared.native_instances;
        let installer = instances.is_installer(owner_key);
        match instances.plugin_requests.try_borrow() {
            Ok(table) if installer => table.view(true),
            _ => AnswerView {
                installer,
                ..AnswerView::default()
            },
        }
    }

    /// Whether the native plugin `id` is one (a request of an interface-6
    /// plugin goes the interface-6 way).
    pub(crate) fn is_native_plugin(&self, id: &str) -> bool {
        self.shared.native_instances.find(id).is_some()
    }

    /// Whether a running native plugin that declared `uses.install` heard of
    /// requests (subscribed to 77): one that may answer.
    fn native_installer_listening(&self) -> bool {
        let instances = &self.shared.native_instances;
        let mut n = 0;
        while let Some((owner, _)) = instances.running_nth(n) {
            n += 1;
            if instances.is_installer(owner)
                && instances.subscribed(owner, wire::EVENT_PLUGINS_REQUESTED_TAG)
            {
                return true;
            }
        }
        false
    }

    /// `command.request_plugins` of the native plugin `by` (`registry.md`
    /// section 5): the ids not on the device go to the installers as
    /// `plugins_requested`; with none able to answer every id is answered
    /// `failed` at once; with none missing nothing is sent.
    pub(crate) fn native_request_plugins(&mut self, by: &Id, plugins: Vec<Id>) {
        let wanted = self.wanted_plugins(plugins);
        if wanted.is_empty() {
            return;
        }
        let installers = self.native_installer_listening();
        let opened = {
            let table = &self.shared.native_instances.plugin_requests;
            table
                .try_borrow_mut()
                .map_err(|_| ERR_FULL)
                .and_then(|mut table| {
                    if installers {
                        table.begin(by.as_str(), &wanted)
                    } else {
                        Ok(table.number_unkept())
                    }
                })
        };
        match opened {
            Ok(request) if installers => {
                if self.native_plugins_requested(by, request, &wanted) == 0 {
                    // Nobody heard it (it could not be held): never left open.
                    self.native_request_lost(by.as_str(), request, &wanted);
                }
            }
            Ok(request) => self.native_answer_failed(by.as_str(), request, &wanted),
            // No room for the request (or the table is busy): answered
            // `failed` at once with request 0, "kept nowhere"; never silently
            // lost.
            Err(_) => self.native_answer_failed(by.as_str(), 0, &wanted),
        }
    }

    /// Answers every id of `wanted` `failed` to `by`.
    fn native_answer_failed(&mut self, by: &str, request: u32, wanted: &[Id]) {
        for plugin in wanted {
            self.native_plugin_answered(by, request, plugin.as_str(), OUTCOME_FAILED);
        }
    }

    /// The request `request` reached no installer: closed, every id `failed`.
    fn native_request_lost(&mut self, by: &str, request: u32, wanted: &[Id]) {
        if let Ok(mut table) = self
            .shared
            .native_instances
            .plugin_requests
            .try_borrow_mut()
        {
            table.discard(request);
        }
        self.native_answer_failed(by, request, wanted);
    }

    /// Holds `event.plugins_requested` for every installer that subscribed;
    /// how many held it.
    fn native_plugins_requested(&mut self, by: &Id, request: u32, wanted: &[Id]) -> usize {
        let mut list = String::new();
        let size: usize = wanted.iter().map(|id| id.as_str().len() + 1).sum();
        if list.try_reserve_exact(size).is_err() {
            return 0;
        }
        for (at, id) in wanted.iter().enumerate() {
            if at > 0 {
                list.push(wire::EVENT_PLUGINS_REQUESTED_PLUGINS_SEPARATOR);
            }
            list.push_str(id.as_str());
        }
        let now_ms = self.now.millis();
        self.native_queue_event_if(
            wire::EVENT_PLUGINS_REQUESTED_TAG,
            &|instances, owner, _| instances.is_installer(owner),
            &|queue| {
                let record = wire::Record::EventPluginsRequested(wire::EventPluginsRequested {
                    now_ms,
                    request,
                    by: by.as_str(),
                    plugins: &list,
                });
                queue.push_record(wire::EVENT_PLUGINS_REQUESTED_TAG, &record)
            },
        )
    }

    /// Holds `event.plugin_answered` for `requester` alone.
    pub(crate) fn native_plugin_answered(
        &mut self,
        requester: &str,
        request: u32,
        plugin: &str,
        outcome: u32,
    ) {
        let now_ms = self.now.millis();
        self.native_queue_event(
            wire::EVENT_PLUGIN_ANSWERED_TAG,
            &|who| who == requester,
            &|queue| {
                let record = wire::Record::EventPluginAnswered(wire::EventPluginAnswered {
                    now_ms,
                    request,
                    plugin,
                    outcome,
                });
                queue.push_record(wire::EVENT_PLUGIN_ANSWERED_TAG, &record)
            },
        );
    }

    /// Applies the answers a callback staged, after its other commands (the
    /// stage checked them against what was open then). A request or id that
    /// is not open any more is dropped; an answer "installed" for a plugin not
    /// on the device is stored as `failed`.
    pub(crate) fn native_apply_answers(&mut self, answers: Vec<Answer>) {
        for answer in answers {
            let outcome =
                if answer.outcome == OUTCOME_INSTALLED && !self.plugin_on_device(&answer.plugin) {
                    OUTCOME_FAILED
                } else {
                    answer.outcome
                };
            let recorded = self
                .shared
                .native_instances
                .plugin_requests
                .try_borrow_mut()
                .map_err(|_| ERR_FULL)
                .and_then(|mut table| table.answer(answer.request, &answer.plugin));
            match recorded {
                Ok(Some(by)) => {
                    self.native_plugin_answered(&by, answer.request, &answer.plugin, outcome);
                }
                Ok(None) => {}
                Err(_) => {
                    self.warn_runtime("a plugin answer was dropped: the host could not keep it")
                }
            }
        }
    }

    /// Closes the requests that nobody can answer any more: with no installer
    /// left that subscribed to 77 every open id is answered `failed`; one whose
    /// requester stopped is dropped. Costs nothing while no request is open.
    pub(crate) fn native_requests_settle(&mut self) {
        let table = &self.shared.native_instances.plugin_requests;
        if table
            .try_borrow()
            .map_or(true, |table| table.open_count() == 0)
        {
            return;
        }
        let listening = self.native_installer_listening();
        loop {
            let instances = &self.shared.native_instances;
            let finished = instances
                .plugin_requests
                .try_borrow_mut()
                .ok()
                .and_then(|mut table| {
                    table.take_finished(|request| {
                        !listening || instances.key_of(request.by()).is_none()
                    })
                });
            let Some(request) = finished else {
                break;
            };
            if self.shared.native_instances.key_of(request.by()).is_some() {
                for plugin in request.open_ids() {
                    self.native_plugin_answered(
                        request.by(),
                        request.number(),
                        plugin,
                        OUTCOME_FAILED,
                    );
                }
            }
        }
    }
}
