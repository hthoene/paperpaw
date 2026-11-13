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
    ui::RequestId as AskId,
};
use paperpaw_ui_kit::mem::{boxed, copy_strings, push};

use crate::native_ask::{self, Decision, Reply, Words};
use crate::native_glue;
use crate::native_index::Index;
use crate::native_installs::Snapshot;
use crate::native_plugin::NativeStore;
use crate::native_queue::Queue;
use crate::native_request::{self, Finished, Last, Request};

/// What is kept from ask to ask, made at the first ask (#248: the store keeps
/// only a pointer until another plugin asks).
fn asks(slot: &mut Option<alloc::boxed::Box<Asks>>) -> Result<&mut Asks, Error> {
    if slot.is_none() {
        *slot = Some(boxed(Asks::default())?);
    }
    slot.as_deref_mut().ok_or(Error::NoMemory)
}

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
    /// An ask for plugins (event 77). Every id the host named gets an outcome
    /// on every path: where the store cannot go on they are *failed*.
    pub(crate) fn plugins_requested(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        request: PluginRequestId,
        by: &str,
        plugins: RequestedPlugins<'_>,
    ) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        let Some(wanted) = self.take_wanted(host, request, plugins)? else {
            return Ok(());
        };
        let asked = (|| {
            let index = Index::kept(&storage)?;
            let open = Request::open(&storage)?.is_some();
            let waiting = Queue::read(&storage)?.requested_waiting();
            let mut words = Words::new(host)?;
            native_ask::asked(
                &storage,
                &mut words,
                index.as_ref(),
                &mut asks(&mut self.asks)?.last,
                (by, &wanted),
                now_ms,
                // The network is found out when the download starts.
                (true, open, waiting),
            )
        })();
        let asked = match asked {
            Ok(asked) => asked,
            Err(_) => {
                return answer_ids(
                    host,
                    request,
                    wanted.iter().map(String::as_str),
                    PluginOutcome::Failed,
                );
            }
        };
        self.answer_stale(host, &asked.stale);
        self.reply_to_ask(host, request, &wanted, asked.reply)
    }

    /// The ids the store takes of an ask; the others are answered here, those
    /// on the device installed, those that are no plugin's name failed. `None`
    /// where the store could not even read what the device has: everything is
    /// answered *failed*.
    fn take_wanted(
        &mut self,
        host: &Host<'_>,
        request: PluginRequestId,
        plugins: RequestedPlugins<'_>,
    ) -> Result<Option<Vec<String>>, Error> {
        let fail_all = |host: &Host<'_>| {
            answer_ids(host, request, plugins.iter(), PluginOutcome::Failed).map(|()| None)
        };
        let mut named: Vec<&str> = Vec::new();
        if named.try_reserve_exact(plugins.len()).is_err() {
            return fail_all(host);
        }
        named.extend(plugins.iter());
        // Only the rows of these plugins are read, not what the device has.
        let Ok(present) = Snapshot::read_some(host, &named) else {
            return fail_all(host);
        };
        let on_device = |id: &str| present.have(id).is_some();
        let Ok(wanted) = native_ask::request_ids(plugins.iter(), &on_device) else {
            return fail_all(host);
        };
        for id in plugins.iter().filter(|id| !wanted.iter().any(|w| w == id)) {
            let outcome = if on_device(id) {
                PluginOutcome::Installed
            } else {
                PluginOutcome::Failed
            };
            answer_ids(host, request, [id], outcome)?;
        }
        Ok(Some(wanted))
    }

    /// The ids of a request an earlier dialog left open are *declined* (the
    /// host keeps a request open until each id has an outcome).
    fn answer_stale(&mut self, host: &Host<'_>, stale: &[String]) {
        if stale.is_empty() {
            return;
        }
        let Some(asks) = self.asks.as_deref_mut() else {
            return;
        };
        asks.waiting.retain(|(request, ids)| {
            if !ids.iter().any(|id| stale.contains(id)) {
                return true;
            }
            let _ = answer_ids(
                host,
                *request,
                ids.iter().map(String::as_str),
                PluginOutcome::Declined,
            );
            false
        });
    }

    fn reply_to_ask(
        &mut self,
        host: &Host<'_>,
        request: PluginRequestId,
        wanted: &[String],
        reply: Reply,
    ) -> Result<(), Error> {
        let all = || wanted.iter().map(String::as_str);
        match reply {
            // Everything asked for is on the device (answered already).
            Reply::Nothing => Ok(()),
            Reply::Join => self.keep_or_fail(host, request, wanted),
            Reply::Answer { ids, outcome } => {
                answer_ids(host, request, ids.iter().map(String::as_str), outcome)
            }
            Reply::AnswerAll(outcome) => answer_ids(host, request, all(), outcome),
            Reply::Dialog(dialog) => {
                answer_ids(
                    host,
                    request,
                    dialog.declined.iter().map(String::as_str),
                    PluginOutcome::Declined,
                )?;
                answer_ids(
                    host,
                    request,
                    dialog.not_found.iter().map(String::as_str),
                    PluginOutcome::NotFound,
                )?;
                if self.keep_waiting(request, &dialog.ids).is_err() {
                    native_request::Request::close(&crate::native_folders::StoreStorage::new(host));
                    return answer_ids(
                        host,
                        request,
                        dialog.ids.iter().map(String::as_str),
                        PluginOutcome::Failed,
                    );
                }
                let asked = {
                    let ui = self.ui(host)?;
                    dialog
                        .question()
                        .and_then(|question| native_glue::confirm(host, ui, question))
                };
                match asked {
                    Ok(id) => {
                        asks(&mut self.asks)?.dialog = Some(id);
                        Ok(())
                    }
                    // The dialog could not be shown: *Not now*.
                    Err(_) => self.decided(host, 0, Decision::NotNow),
                }
            }
            Reply::Fetch(next) => {
                self.keep_or_fail(host, request, wanted)?;
                self.carry_fetch(host, next)
            }
        }
    }

    /// Keeps `request` to be answered when the plugins are; where it cannot be
    /// kept every id is *failed* at once.
    fn keep_or_fail(
        &mut self,
        host: &Host<'_>,
        request: PluginRequestId,
        ids: &[String],
    ) -> Result<(), Error> {
        if self.keep_waiting(request, ids).is_ok() {
            return Ok(());
        }
        answer_ids(
            host,
            request,
            ids.iter().map(String::as_str),
            PluginOutcome::Failed,
        )
    }

    fn keep_waiting(&mut self, request: PluginRequestId, ids: &[String]) -> Result<(), Error> {
        push(
            &mut asks(&mut self.asks)?.waiting,
            (request, copy_strings(ids)?),
        )
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
        if let Some(asks) = self.asks.as_deref_mut() {
            asks.dialog = None;
        }
        let decision = native_ask::decision(answer.outcome(), answer.choice(), false);
        self.decided(host, now_ms, decision)
    }

    fn decided(&mut self, host: &Host<'_>, now_ms: u64, decision: Decision) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        let last = &mut asks(&mut self.asks)?.last;
        let reply = native_ask::answered(&storage, last, now_ms, true, decision)?;
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
        let waiting = self
            .asks
            .as_deref_mut()
            .map(|asks| core::mem::take(&mut asks.waiting))
            .unwrap_or_default();
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
        let waiting = self
            .asks
            .as_deref_mut()
            .map(|asks| core::mem::take(&mut asks.waiting))
            .unwrap_or_default();
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
        let storage = crate::native_folders::StoreStorage::new(host);
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
