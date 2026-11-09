//! What the kernel's event pump asks of a host service that raises events
//! to plugins: `core.net` (99-102) and `core.sockets` (106, 107) today.
//!
//! The service queues its events as the shell reports (`NetReports`,
//! `SockReports`); it never calls a plugin. The kernel, between callbacks,
//! asks [`ServiceEvents::next_event`] for the oldest event of one plugin,
//! hands it to the plugin's callback, and calls
//! [`ServiceEvents::event_delivered`] when that returns, also when it failed
//! ("never dropped" means a busy plugin gets the event later, not that a failed
//! callback gets it again). What the plugin may read of the event (a finished
//! request's answer, a socket's bytes) is readable from the start of that
//! callback to its return and not after.
use super::{net_service, sock_service};

/// The largest encoded event of a service: the served request of `core.net`
/// (a 16-byte method, a 1,024-byte target and the fixed fields).
pub(crate) const EVENT_BUFFER: usize = 1_280;

/// A host service with events for plugins. Every call is a plain hook of
/// the service's table: `ERR_BUSY` when the table is borrowed (call again
/// on the next pass).
pub(crate) trait ServiceEvents {
    /// The oldest event of `owner`, encoded into `out` as a one-record batch;
    /// its length and the work run it belongs to (`None`: no run; the kernel
    /// delivers the events of a run's requests and sockets inside that run),
    /// or `None` when there is no event. A failure consumes nothing.
    ///
    /// # Errors
    /// `ERR_BUSY` or `ERR_NO_SPACE`.
    fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<(usize, Option<u64>)>, i32>;

    /// The callback of `owner` returned: what it could read is freed, and a
    /// request, socket or broadcast whose last event that was ends its work
    /// run (the run counts it until then).
    ///
    /// # Errors
    /// `ERR_BUSY`.
    fn event_delivered(&self, owner: u64) -> Result<(), i32>;

    /// A screen instance of `owner` closed: what it started ends.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    fn screen_closed(&self, owner: u64, screen: u64) -> Result<(), i32>;

    /// A work run of `owner` reached its longest time: what it started ends.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    fn work_ended(&self, owner: u64, run: u64) -> Result<(), i32>;

    /// The clock: what ran out of time ends, what can start starts.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    fn tick(&self) -> Result<(), i32>;

    /// When the first thing in the service runs out of time (the clock of the
    /// host that serves it, in milliseconds); `None` when nothing waits on
    /// the clock. A borrowed table answers `Some(0)`: look again.
    fn next_deadline(&self) -> Option<u64>;
}

impl ServiceEvents for net_service::NetServices {
    fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<(usize, Option<u64>)>, i32> {
        net_service::next_event_run(&*self.0, owner, out)
    }
    fn event_delivered(&self, owner: u64) -> Result<(), i32> {
        net_service::event_delivered(&*self.0, owner)
    }
    fn screen_closed(&self, owner: u64, screen: u64) -> Result<(), i32> {
        net_service::screen_closed(&*self.0, owner, screen)
    }
    fn work_ended(&self, owner: u64, run: u64) -> Result<(), i32> {
        net_service::work_ended(&*self.0, owner, run)
    }
    fn tick(&self) -> Result<(), i32> {
        net_service::tick(&*self.0)
    }
    fn next_deadline(&self) -> Option<u64> {
        net_service::next_deadline(&*self.0)
    }
}

impl ServiceEvents for sock_service::SockServices {
    fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<(usize, Option<u64>)>, i32> {
        sock_service::next_event_run(&*self.0, owner, out)
    }
    fn event_delivered(&self, owner: u64) -> Result<(), i32> {
        sock_service::event_delivered(&*self.0, owner)
    }
    fn screen_closed(&self, owner: u64, screen: u64) -> Result<(), i32> {
        sock_service::screen_closed(&*self.0, owner, screen)
    }
    fn work_ended(&self, owner: u64, run: u64) -> Result<(), i32> {
        sock_service::work_ended(&*self.0, owner, run)
    }
    fn tick(&self) -> Result<(), i32> {
        sock_service::tick(&*self.0)
    }
    fn next_deadline(&self) -> Option<u64> {
        sock_service::next_deadline(&*self.0)
    }
}
