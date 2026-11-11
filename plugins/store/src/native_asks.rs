//! Plugins other plugins ask for (events 77 and 78, `docs/abi/registry.md` 4.2),
//! on the host: the ask is decided by `native_ask`, this carries it out. The
//! reader is asked once (`Ui::ask`), the answer to each id of each request goes
//! out with `command.answer_plugin` (needs `uses.install`); what is held between
//! the ask and the answers is the host's request numbers and the ids each
//! stands for, a few bytes. A request the store was restarted under is failed by
//! the host.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, PluginOutcome},
    events::{Answered, PluginRequestId, RequestedPlugins},
    storage::DataStorage,
    ui::RequestId as AskId,
};
use paperpaw_ui_kit::mem::{copy_strings, push};

use crate::native_ask::{self, Decision, Reply, Words};
use crate::native_glue;
use crate::native_index::Index;
use crate::native_plugin::NativeStore;
use crate::native_queue::Queue;
use crate::native_request::{self, Finished, Last, Request};

/// What the store keeps from ask to ask.
#[derive(Debug, Default)]
pub(crate) struct Asks {
    /// The last request shown, for a repeated ask.
    last: Option<Last>,
    /// The host's requests still to be answered, each with the ids it asked for.
    waiting: Vec<(PluginRequestId, Vec<String>)>,
    /// The dialog on the screen, if one is.
    dialog: Option<AskId>,
}

impl Asks {
    /// Whether `request` is the dialog of an ask.
    pub(crate) fn is_dialog(&self, request: AskId) -> bool {
        self.dialog == Some(request)
    }
}

/// Answers `ids` of `request` with `outcome`. A request the host no longer has
/// open (every id answered, a restart) is no error.
fn answer_ids<'a>(
    host: &Host<'_>,
    request: PluginRequestId,
    ids: impl IntoIterator<Item = &'a str>,
    outcome: PluginOutcome,
) -> Result<(), Error> {
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(host, &mut scratch);
    for id in ids {
        if let Err(Error::NoMemory) = sender.answer_plugin(request, id, outcome) {
            return Err(Error::NoMemory);
        }
    }
    Ok(())
}

impl NativeStore {
    /// An ask for plugins (event 77).
    pub(crate) fn plugins_requested(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        request: PluginRequestId,
        by: &str,
        plugins: RequestedPlugins<'_>,
    ) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        self.ensure_snapshot(host)?;
        let (wanted, left_out) = {
            let snapshot = self.snapshot.as_ref().ok_or(Error::State)?;
            let on_device = |id: &str| snapshot.have(id).is_some();
            let wanted = native_ask::request_ids(plugins.iter(), &on_device)?;
            // What the host named and the store does not take: on the device
            // already, or no plugin's name.
            let mut left_out: Vec<(String, PluginOutcome)> = Vec::new();
            for id in plugins.iter().filter(|id| !wanted.iter().any(|w| w == id)) {
                let outcome = if on_device(id) {
                    PluginOutcome::Installed
                } else {
                    PluginOutcome::Failed
                };
                push(&mut left_out, (native_glue::own(id)?, outcome))?;
            }
            (wanted, left_out)
        };
        for (id, outcome) in &left_out {
            answer_ids(host, request, [id.as_str()], *outcome)?;
        }
        let index = Index::kept(&storage)?;
        let open = Request::open(&storage)?.is_some();
        let waiting = Queue::read(&storage)?.requested_waiting();
        let reply = {
            let mut words = Words::new(host)?;
            native_ask::asked(
                &storage,
                &mut words,
                index.as_ref(),
                &mut self.asks.last,
                (by, &wanted),
                now_ms,
                // The network is found out when the download starts.
                (true, open, waiting),
            )?
        };
        let result = self.reply_to_ask(host, request, &wanted, reply);
        if self.screens_empty() {
            self.forget_snapshot();
        }
        result
    }

    fn reply_to_ask(
        &mut self,
        host: &Host<'_>,
        request: PluginRequestId,
        wanted: &[String],
        reply: Reply,
    ) -> Result<(), Error> {
        match reply {
            Reply::Nothing => {
                // Everything asked for is on the device.
                answer_ids(
                    host,
                    request,
                    wanted.iter().map(String::as_str),
                    PluginOutcome::Installed,
                )
            }
            Reply::Join => self.keep_waiting(request, wanted),
            Reply::Answer { ids, outcome } => {
                answer_ids(host, request, ids.iter().map(String::as_str), outcome)
            }
            Reply::Dialog(dialog) => {
                // The library lacks what the dialog does not name.
                let lacking = wanted.iter().filter(|id| !dialog.ids.contains(id));
                answer_ids(
                    host,
                    request,
                    lacking.map(String::as_str),
                    PluginOutcome::NotFound,
                )?;
                self.keep_waiting(request, &dialog.ids)?;
                let asked = {
                    let ui = self.ui(host)?;
                    dialog
                        .question()
                        .and_then(|question| native_glue::confirm(host, ui, question))
                };
                match asked {
                    Ok(id) => {
                        self.asks.dialog = Some(id);
                        Ok(())
                    }
                    Err(Error::NoMemory) => Err(Error::NoMemory),
                    // The dialog could not be shown: *Not now*.
                    Err(_) => self.decided(host, 0, Decision::NotNow),
                }
            }
            Reply::Fetch(next) => {
                self.keep_waiting(request, wanted)?;
                self.carry_fetch(host, next)
            }
        }
    }

    fn keep_waiting(&mut self, request: PluginRequestId, ids: &[String]) -> Result<(), Error> {
        push(&mut self.asks.waiting, (request, copy_strings(ids)?))
    }

    /// The reader answered the dialog of an ask.
    pub(crate) fn dialog_answered(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        answer: &Answered,
    ) -> Result<(), Error> {
        use paperpaw_sdk::native::events::AnswerOutcome;
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asks.dialog = None;
        let decision = native_ask::decision(answer.outcome(), answer.choice(), false);
        self.decided(host, now_ms, decision)
    }

    fn decided(&mut self, host: &Host<'_>, now_ms: u64, decision: Decision) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        let reply = native_ask::answered(&storage, &mut self.asks.last, now_ms, true, decision)?;
        match reply {
            Reply::Answer { ids, outcome } => {
                self.answer_waiting(host, ids.iter().map(String::as_str), outcome)
            }
            Reply::Fetch(next) => self.carry_fetch(host, next),
            _ => Ok(()),
        }
    }

    /// The plugins are queued: the queue starts and the screen shows the
    /// download.
    fn carry_fetch(
        &mut self,
        host: &Host<'_>,
        next: crate::native_screens::Next,
    ) -> Result<(), Error> {
        if let crate::native_screens::Next::Open { screen, params } = next {
            let ui = self.ui(host)?;
            native_glue::open(host, ui, screen, &params)?;
        }
        self.go_on(host)
    }

    /// Tells every request waiting what became of `ids`, and lets them go.
    fn answer_waiting<'a>(
        &mut self,
        host: &Host<'_>,
        ids: impl Iterator<Item = &'a str> + Clone,
        outcome: PluginOutcome,
    ) -> Result<(), Error> {
        let waiting = core::mem::take(&mut self.asks.waiting);
        for (request, asked) in &waiting {
            let mine = ids.clone().filter(|id| asked.iter().any(|a| a == id));
            answer_ids(host, *request, mine, outcome)?;
        }
        Ok(())
    }

    /// The queue ended: each plugin a request asked for is installed or not.
    pub(crate) fn requests_finished(
        &mut self,
        host: &Host<'_>,
        done: &Finished,
    ) -> Result<(), Error> {
        let answers = native_ask::finished_answers(done)?;
        let waiting = core::mem::take(&mut self.asks.waiting);
        for (request, asked) in &waiting {
            for (id, outcome) in &answers {
                if asked.iter().any(|a| a == id) {
                    answer_ids(host, *request, [*id], *outcome)?;
                }
            }
        }
        Ok(())
    }

    /// The download of what a request asked for could not start (no network):
    /// the request is closed and each id failed.
    pub(crate) fn requests_failed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        let Some(request) = native_request::Request::open(&storage)? else {
            return Ok(());
        };
        native_request::Request::close(&storage);
        self.answer_waiting(
            host,
            request.ids.iter().map(String::as_str),
            PluginOutcome::Failed,
        )
    }
}
