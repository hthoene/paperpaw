//! "The network is lost", as the firmware tells it to the host services
//! (`core.sockets`, `core.net`): two flags, not an item in a list. When the
//! firmware has run short of memory (the very case in which it ends every
//! socket and request) nothing may be allocated to say so, and a report that
//! core refuses as busy must be made again; so the report is a flag that is
//! raised, taken first at the next report, and put back where it was refused.

/// A service that can be told the network is lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Service {
    /// `core.sockets`.
    Sockets,
    /// `core.net`.
    Requests,
}

/// Which services still have to be told. Two bytes, never allocated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetLost {
    sockets: bool,
    requests: bool,
}

impl NetLost {
    /// Nothing to tell.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sockets: false,
            requests: false,
        }
    }

    /// `core.sockets` is to be told (once, however often this is raised).
    pub fn raise_sockets(&mut self) {
        self.sockets = true;
    }

    /// `core.net` is to be told.
    pub fn raise_requests(&mut self) {
        self.requests = true;
    }

    /// Whether anything is to be told.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.sockets || self.requests
    }

    /// What is to be told after `other` was taken out and not all of it told:
    /// both sets.
    pub fn merge(&mut self, other: Self) {
        self.sockets |= other.sockets;
        self.requests |= other.requests;
    }

    /// Takes what is to be told, leaving nothing.
    pub fn take(&mut self) -> Self {
        core::mem::take(self)
    }

    /// Tells each service that is to be told, sockets first. `tell` returns
    /// `true` when the service took it; one that did not (core's table was
    /// busy) stays raised, for the next report. Whether any was told.
    pub fn report(&mut self, mut tell: impl FnMut(Service) -> bool) -> bool {
        let mut told = false;
        if self.sockets && tell(Service::Sockets) {
            self.sockets = false;
            told = true;
        }
        if self.requests && tell(Service::Requests) {
            self.requests = false;
            told = true;
        }
        told
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn needs_no_memory_to_be_raised() {
        // Two bytes on the stack: raising it cannot fail for want of memory.
        assert_eq!(core::mem::size_of::<NetLost>(), 2);
        let mut lost = NetLost::new();
        lost.raise_sockets();
        lost.raise_sockets();
        lost.raise_requests();
        assert!(lost.any());
    }

    #[test]
    fn sockets_are_told_first_and_once() {
        let mut lost = NetLost::new();
        lost.raise_requests();
        lost.raise_sockets();
        let mut order = Vec::new();
        assert!(lost.report(|service| {
            order.push(service);
            true
        }));
        assert_eq!(order, [Service::Sockets, Service::Requests]);
        assert!(!lost.any());
        assert!(!lost.report(|_| panic!("told twice")));
    }

    #[test]
    fn a_busy_service_is_told_again_and_the_other_is_not_held_up() {
        let mut lost = NetLost::new();
        lost.raise_sockets();
        lost.raise_requests();
        assert!(lost.report(|service| service == Service::Requests));
        assert!(lost.any());
        let mut again = Vec::new();
        assert!(lost.report(|service| {
            again.push(service);
            true
        }));
        assert_eq!(again, [Service::Sockets]);
        assert!(!lost.any());
    }

    #[test]
    fn taken_flags_are_merged_back_where_refused() {
        let mut lost = NetLost::new();
        lost.raise_sockets();
        let mut taken = lost.take();
        assert!(!lost.any());
        // Raised meanwhile, e.g. by a step during the report.
        lost.raise_requests();
        assert!(!taken.report(|_| false));
        lost.merge(taken);
        assert!(lost.any());
        let mut seen = Vec::new();
        lost.report(|service| {
            seen.push(service);
            true
        });
        assert_eq!(seen, [Service::Sockets, Service::Requests]);
    }
}
