//! Plugin requests of native plugins (`docs/abi/registry.md` section 4.2 and
//! 5): `command.request_plugins` and `event.plugins_requested` (77), and the
//! answer `event.plugin_answered` (78), told to the requester only.
//!
//! A request never installs anything: it asks the plugins that declared
//! `uses.install` (and subscribed to 77), which ask the user. It is native
//! only: a native plugin asks native installers; interface-6 plugins keep
//! their own bus path until activation.
//!
//! * **Lazy.** The table of open requests is an empty `Vec` until the first
//!   request: a device that never asks costs nothing.
//! * **Bounded.** At most [`MAX_OPEN`] requests are open device-wide (`ERR_FULL`
//!   at submit beyond); the table grows by a fallible reserve. (A request is
//!   closed by `answer_plugin`, which comes with the next change.)
//! * **Ids already on the device are dropped**; with none left nothing is
//!   sent. With no installer able to answer, every id is answered `failed` at
//!   once.
//! * **Edges.** 77 and 78 are held encoded in the plugins' queues
//!   (`core_events`, `EDGE_TAGS`), never coalesced; one that does not fit is
//!   dropped and logged.
use super::registrations as wire;
use crate::Kernel;
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_NO_MEMORY};
use paperpaw_sdk::Id;

/// The most requests open device-wide.
pub(crate) const MAX_OPEN: usize = 4;
/// `event.plugin_answered.outcome`: the plugin could not be had.
pub(crate) const OUTCOME_FAILED: u32 = 2;

/// An open request (its ids and answers come with `answer_plugin`).
pub(crate) struct Request {
    pub(crate) number: u32,
}

/// The open requests.
#[derive(Default)]
pub(crate) struct PluginRequests {
    open: Vec<Request>,
    last: u32,
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

    /// Opens a request.
    ///
    /// # Errors
    /// `ERR_FULL` with [`MAX_OPEN`] open, `ERR_NO_MEMORY`.
    pub(crate) fn begin(&mut self) -> Result<u32, i32> {
        if self.open.len() >= MAX_OPEN {
            return Err(ERR_FULL);
        }
        self.open.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let number = self.number();
        self.open.push(Request { number });
        Ok(number)
    }
}

impl Kernel {
    /// The ids of `plugins` not on the device, each once.
    fn wanted_plugins(&self, plugins: Vec<Id>) -> Vec<Id> {
        let mut wanted: Vec<Id> = Vec::new();
        for id in plugins {
            let known = self.shared.plugins.get(id.as_str()).is_some()
                || self.is_native_plugin(id.as_str())
                || id.as_str() == crate::core_plugin::CORE.as_str();
            if !known && !wanted.contains(&id) {
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

    /// Whether the native plugin `id` is one (a request of an interface-6
    /// plugin goes the interface-6 way).
    pub(crate) fn is_native_plugin(&self, id: &str) -> bool {
        self.shared.native_instances.find(id).is_some()
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
        let installers = {
            let instances = &self.shared.native_instances;
            let mut n = 0;
            let mut found = false;
            while let Some((owner, _)) = instances.running_nth(n) {
                n += 1;
                found |= instances.is_installer(owner)
                    && instances.subscribed(owner, wire::EVENT_PLUGINS_REQUESTED_TAG);
            }
            found
        };
        let opened = if installers {
            self.shared
                .native_instances
                .plugin_requests
                .try_borrow_mut()
                .map_err(|_| ERR_FULL)
                .and_then(|mut table| table.begin())
        } else {
            self.shared
                .native_instances
                .plugin_requests
                .try_borrow_mut()
                .map(|mut table| table.number_unkept())
                .map_err(|_| ERR_FULL)
        };
        match opened {
            Ok(request) if installers => self.native_plugins_requested(by, request, &wanted),
            Ok(request) => {
                for plugin in &wanted {
                    self.native_plugin_answered(
                        by.as_str(),
                        request,
                        plugin.as_str(),
                        OUTCOME_FAILED,
                    );
                }
            }
            Err(_) => {
                // No room for the request (or the table is busy): answered
                // `failed` at once, never silently lost.
                for plugin in &wanted {
                    self.native_plugin_answered(by.as_str(), 0, plugin.as_str(), OUTCOME_FAILED);
                }
            }
        }
    }

    /// Holds `event.plugins_requested` for every installer that subscribed.
    fn native_plugins_requested(&mut self, by: &Id, request: u32, wanted: &[Id]) {
        let mut list = String::new();
        let size: usize = wanted.iter().map(|id| id.as_str().len() + 1).sum();
        if list.try_reserve_exact(size).is_err() {
            return;
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
        );
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
}
