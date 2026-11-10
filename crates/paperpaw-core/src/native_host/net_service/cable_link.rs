//! The cable (USB) as the transport of a native plugin's `net_serve` port 0
//! (`network.md` 5.1, the ruling of todo 384 on #228): the computer's lines
//! (`docs/usb-transfer.md`, protocol 1) reach the server through the kernel,
//! never through the shell's HTTP layer, which only passes the lines on.
//!
//! The link has two sides:
//!
//! * **The service's** ([`super::device::NetDevice`], under the table's lock):
//!   `listen` and `unlisten` of port 0, and the `reply` and `close_served` of
//!   a request the cable carries. Nothing here calls back into the service: a
//!   start to report and the requests answered wait in the link.
//! * **The kernel's** (`native_host::cable_pump`, no lock held): the lines
//!   the shell passes (`Kernel::cable_line`), the start reported, the answered
//!   requests reported gone, and the lines to send (`Kernel::take_cable_lines`).
//!
//! The protocol's state (the requests open, the body coming, the lines to
//! send) is the kernel's [`Cable`]. Its numbers for requests ("link
//! numbers") are not the service's: each request open on the cable maps one
//! to the other ([`Open`]). At most one server listens on the cable; it uses
//! no socket and no pool buffer, so it does not count against the profile's
//! `SERVERS`.
use crate::io::cable::Cable;
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_UNSUPPORTED};

/// How long a request without a body waits for the cable's server to start
/// (the screen of `core.cable` opening and serving), in ms.
pub(crate) const HOLD_MS: u64 = 5_000;

/// A request open on the cable: its link number, the service's number, where
/// its body goes and how much of it came.
#[derive(Debug)]
pub(crate) struct Open {
    pub(crate) link: u32,
    pub(crate) request: u32,
    pub(crate) path: String,
    pub(crate) received: u64,
}

/// A request without a body that came while no server was started: it goes to
/// the server once it is, or is answered 503 at `until`.
#[derive(Debug)]
pub(crate) struct Held {
    pub(crate) n: u32,
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) until: u64,
}

#[derive(Debug, Default)]
struct Link {
    /// The service's key of the server on the cable.
    server: Option<u32>,
    /// `listen` answered; the kernel reports `started` next.
    to_start: bool,
    started: bool,
    /// The plugin stopped the cable's server (its screen closed): a request
    /// before the protocol's pause gets 403.
    pub(crate) ended: bool,
    proto: Cable,
    open: Vec<Open>,
    /// Requests answered or closed on the cable: the kernel tells the service
    /// they are gone.
    gone: Vec<u32>,
    held: Option<Held>,
}

/// See the module.
#[derive(Debug, Default)]
pub(crate) struct CableLink(RefCell<Link>);

impl CableLink {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut Link) -> T) -> Result<T, i32> {
        let mut link = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        Ok(f(&mut link))
    }

    // The service's side.

    /// `listen` on port 0: `false` when a server listens on the cable already
    /// (the service tells the plugin `failed`, `refused`).
    pub(crate) fn listen(&self, server: u32) -> bool {
        self.with(|link| {
            if link.server.is_some() {
                return false;
            }
            link.server = Some(server);
            link.to_start = true;
            link.started = false;
            link.ended = false;
            true
        })
        .unwrap_or(false)
    }

    /// `unlisten`: the requests open on the cable are answered 503 and the
    /// server is gone. A key that is not the cable's is not touched.
    pub(crate) fn unlisten(&self, server: u32) -> bool {
        self.with(|link| {
            if link.server != Some(server) {
                return false;
            }
            link.server = None;
            link.to_start = false;
            link.started = false;
            link.ended = true;
            for open in core::mem::take(&mut link.open) {
                link.proto
                    .refuse_request(open.link, 503, "the server stopped");
                // `unlisten` runs inside the service's stop, which forgets
                // the server's requests itself.
            }
            if let Some(held) = link.held.take() {
                link.proto.refuse(held.n, 503, "nothing serves the cable");
            }
            true
        })
        .unwrap_or(false)
    }

    /// Whether the service's request `request` is the cable's.
    pub(crate) fn carries(&self, request: u32) -> bool {
        // Link borrows are never nested and no callback runs under one, so
        // this borrow is had: were it refused, the service would take the
        // cable's request for a network one (`NetDevice::on_cable`).
        debug_assert!(
            self.0.try_borrow_mut().is_ok(),
            "the cable link is borrowed"
        );
        self.with(|link| link.open.iter().any(|o| o.request == request))
            .unwrap_or(false)
    }

    /// The plugin's answer to `request`. A file to send is refused on the
    /// cable, as in Beta 3: the computer gets the refusal and the plugin
    /// `ERR_UNSUPPORTED`.
    ///
    /// # Errors
    /// `ERR_UNSUPPORTED` for a file; `ERR_BUSY` while the answers waiting
    /// for the computer leave no room (`Cable::has_room`; the request stays
    /// open, so the plugin may answer again) or the link is borrowed;
    /// `ERR_NO_MEMORY` when the body cannot be kept to send.
    pub(crate) fn answer(
        &self,
        request: u32,
        status: u32,
        content_type: &str,
        body: Option<&[u8]>,
        file: bool,
    ) -> Result<(), i32> {
        self.with(|link| {
            let at = link.open.iter().position(|o| o.request == request)?;
            // A full queue empties as the computer reads: the plugin retries.
            let len = if file {
                64
            } else {
                body.map_or(0, <[u8]>::len)
            };
            if !link.proto.has_room(len) {
                return Some(Err(ERR_BUSY));
            }
            let open = link.open.remove(at);
            // Room reserved when it was kept (`keep`): no allocation here.
            link.gone.push(request);
            let mut kept = Vec::new();
            let fits = body.is_none_or(|b| kept.try_reserve_exact(b.len()).is_ok());
            if let (true, Some(b)) = (fits, body) {
                kept.extend_from_slice(b);
            }
            let status = u16::try_from(status).unwrap_or(500);
            Some(if file {
                link.proto
                    .answer(open.link, status, content_type, Vec::new(), true);
                Err(ERR_UNSUPPORTED)
            } else if fits {
                // No memory for the answer's lines (room was checked above).
                if link
                    .proto
                    .answer(open.link, status, content_type, kept, false)
                {
                    Ok(())
                } else {
                    Err(ERR_NO_MEMORY)
                }
            } else {
                link.proto
                    .refuse_request(open.link, 503, "no memory for the answer");
                Err(ERR_NO_MEMORY)
            })
        })?
        .unwrap_or(Ok(()))
    }

    /// Close `request` with a status of the host's own (503 when the plugin
    /// was too slow), or without an answer (0, answered 503 on the cable,
    /// where every request gets an answer).
    pub(crate) fn close(&self, request: u32, status: u32) {
        let _ = self.with(|link| {
            if let Some(at) = link.open.iter().position(|o| o.request == request) {
                let open = link.open.remove(at);
                let status = match u16::try_from(status) {
                    Ok(s) if s >= 100 => s,
                    _ => 503,
                };
                link.proto.refuse_request(open.link, status, "");
                link.gone.push(request);
            }
        });
    }

    // The kernel's side.

    /// The server to report started, once.
    pub(crate) fn take_start(&self) -> Option<u32> {
        self.with(|link| {
            if !core::mem::take(&mut link.to_start) {
                return None;
            }
            link.started = true;
            link.server
        })
        .ok()
        .flatten()
    }

    /// A start the service did not take (the server stopped meanwhile, its
    /// stop `unlisten`ed it). The reader ended nothing on the device, so the
    /// next request opens `core.cable` again instead of answering 403.
    pub(crate) fn start_refused(&self, server: u32) {
        let _ = self.with(|link| {
            if link.server.is_none_or(|s| s == server) {
                link.server = None;
                link.started = false;
                link.ended = false;
            }
        });
    }

    /// The server on the cable, once its start was reported.
    pub(crate) fn started(&self) -> Option<u32> {
        self.with(|link| link.server.filter(|_| link.started))
            .ok()
            .flatten()
    }

    /// Whether the plugin ended the cable on the device.
    pub(crate) fn ended(&self) -> bool {
        self.with(|link| link.ended).unwrap_or(false)
    }

    /// A request the service answered or closed, to report gone.
    pub(crate) fn pop_gone(&self) -> Option<u32> {
        self.with(|link| link.gone.pop()).ok().flatten()
    }

    /// The protocol's state, for the kernel's handling of a line.
    pub(crate) fn proto<T>(&self, f: impl FnOnce(&mut Cable) -> T) -> Result<T, i32> {
        self.with(|link| f(&mut link.proto))
    }

    /// Keeps the request `open` the service accepted.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (the caller ends the request).
    pub(crate) fn keep(&self, open: Open) -> Result<(), i32> {
        self.with(|link| {
            link.open.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            // Each open request becomes at most one `gone`: its room now.
            let gone = link.open.len() + 1;
            link.gone.try_reserve(gone).map_err(|_| ERR_NO_MEMORY)?;
            link.open.push(open);
            Ok(())
        })?
    }

    /// The open request of link number `link`: `visit` gets it.
    pub(crate) fn with_open<T>(&self, link: u32, visit: impl FnOnce(&mut Open) -> T) -> Option<T> {
        self.with(|l| l.open.iter_mut().find(|o| o.link == link).map(visit))
            .ok()
            .flatten()
    }

    /// Forgets the open request of link number `link`: its service number.
    pub(crate) fn forget(&self, link: u32) -> Option<u32> {
        self.with(|l| {
            let at = l.open.iter().position(|o| o.link == link)?;
            Some(l.open.remove(at).request)
        })
        .ok()
        .flatten()
    }

    /// Holds `held`; one held before is answered 503 (the computer gave it
    /// up by asking again).
    pub(crate) fn hold(&self, held: Held) {
        let _ = self.with(|link| {
            if let Some(old) = link.held.replace(held) {
                link.proto.refuse(old.n, 503, "nothing serves the cable");
            }
        });
    }

    /// The held request, if the server has started or its time ran out
    /// (`Err`: answered 503 here).
    pub(crate) fn take_held(&self, now: u64) -> Option<Held> {
        self.with(|link| {
            let started = link.started && link.server.is_some();
            match link.held.take() {
                Some(held) if started => Some(held),
                Some(held) if now >= held.until => {
                    link.proto.refuse(held.n, 503, "nothing serves the cable");
                    None
                }
                other => {
                    link.held = other;
                    None
                }
            }
        })
        .ok()
        .flatten()
    }

    /// When the held request runs out of time.
    pub(crate) fn held_until(&self) -> Option<u64> {
        self.with(|link| link.held.as_ref().map(|h| h.until))
            .ok()
            .flatten()
    }

    /// The next line to send to the computer.
    pub(crate) fn next_line(&self) -> Option<String> {
        self.with(|link| link.proto.next_line()).ok().flatten()
    }
}

#[cfg(test)]
#[path = "cable_link_tests.rs"]
mod tests;
