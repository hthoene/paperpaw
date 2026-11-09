//! The kernel's side of the cable's native server (`net_service::cable_link`,
//! the ruling of todo 384 on #228): the computer's lines go to the plugin that
//! serves `net_serve` port 0, under the same rules as a request over the
//! network (`network.md` 5.1-5.3: `max_body`, the body in
//! `.received/<server>/<request>`, 507 without room, 503 for a slow plugin).
//!
//! * **No server yet:** a request opens `core.cable` (as interface 6 did). A
//!   request without a body is held until the server has started, at most
//!   [`HOLD_MS`]; one with a body is answered 503 at once, since its data
//!   lines follow without waiting and are not kept.
//! * **After *Back*:** the plugin stopped the server; a request before the
//!   protocol's pause is answered 403 "ended on the device".
//! * **Answers** go back as `PPT/1` lines (`Kernel::take_cable_lines`); a file
//!   to send is refused on the cable, as in Beta 3.
//!
//! An interface-6 server on the cable, while one runs, keeps the lines
//! (`network.rs`).
use super::net_service::{
    self, Decision, NetDevice,
    cable_link::{CableLink, HOLD_MS, Held, Open},
};
use super::shell_wiring::ServedDecision;
use crate::Kernel;
use crate::io::cable::Line;
use alloc::rc::Rc;
use paperpaw_sdk::{Params, builtin::slots};

/// The kernel's answer to a request the cable's server cannot take.
fn refuse_link(link: &CableLink, number: u32, status: u32, why: &str) {
    let status = u16::try_from(status).unwrap_or(503);
    let _ = link.proto(|c| c.refuse_request(number, status, why));
}

impl Kernel {
    /// `core.net`'s device for the cable's lines, while no interface-6 server
    /// runs on the cable.
    pub(crate) fn native_cable(&self) -> Option<Rc<NetDevice>> {
        if self.io.servers.cable_server().is_some() {
            return None;
        }
        self.shared
            .native_instances
            .hosts()?
            .cable_net()
            .filter(|device| device.cable().is_some())
            .cloned()
    }

    /// `core.net`'s device where it serves the cable, interface-6 server or
    /// not (the lines it queued still go out).
    fn cable_device(&self) -> Option<Rc<NetDevice>> {
        self.shared.native_instances.hosts()?.cable_net().cloned()
    }

    /// A line of the computer's for the native server. `paused`: the last
    /// line came long enough ago that a request opens the screen again.
    /// A request the screen it opened serves through interface 6 is given
    /// back (`Some`), for `network.rs` to take.
    pub(crate) fn native_cable_line(
        &mut self,
        device: &NetDevice,
        line: Line,
        paused: bool,
    ) -> Option<Line> {
        let link = device.cable()?;
        match line {
            Line::Request {
                n,
                method,
                target,
                len,
            } => {
                if link.started().is_none() {
                    if link.ended() && !paused {
                        let _ = link.proto(|c| c.refuse_upload(n, len, 403, "ended on the device"));
                        return None;
                    }
                    if link.held_until().is_none() {
                        self.output
                            .commands
                            .push(crate::Command::OpenSlot(slots::CABLE, Params::new()));
                        self.run_commands();
                        if self.io.servers.cable_server().is_some() {
                            return Some(Line::Request {
                                n,
                                method,
                                target,
                                len,
                            });
                        }
                    }
                    // A screen that served at once has its start told now.
                    self.native_cable_pump();
                }
                match link.started() {
                    Some(server) => self.cable_deliver(device, server, n, &method, &target, len),
                    None if len == 0 => link.hold(Held {
                        n,
                        method,
                        target,
                        until: self.now.millis().saturating_add(HOLD_MS),
                    }),
                    None => {
                        let _ = link
                            .proto(|c| c.refuse_upload(n, len, 503, "nothing serves the cable"));
                    }
                }
            }
            Line::Data { n, bytes } => self.cable_data(device, n, &bytes),
            Line::Bad { n: Some(n), why } => {
                if let Ok(Some(number)) = link.proto(|c| c.cut(n))
                    && let Some(request) = link.forget(number)
                {
                    let _ = net_service::served_gone(device, request);
                }
                let _ = link.proto(|c| c.refuse(n, 400, why));
            }
            Line::Bad { n: None, .. } => {}
        }
        None
    }

    /// The request `n` of the computer goes to the cable's `server`.
    fn cable_deliver(
        &mut self,
        device: &NetDevice,
        server: u32,
        n: u32,
        method: &str,
        target: &str,
        len: u64,
    ) {
        let Some(link) = device.cable() else {
            return;
        };
        let Ok((number, cut, replaced)) = link.proto(|c| c.begin(n, len)) else {
            return;
        };
        // An earlier request by this number, given up by the computer: it
        // ends on the service (an answer numbered `n` is the new one's).
        if let Some(replaced) = replaced
            && let Some(request) = link.forget(replaced)
        {
            let _ = net_service::served_gone(device, request);
        }
        // A body still coming, cut short by this request: let go, and told,
        // unless the computer gave this request its number (it gave the
        // other up).
        if let Some((m, cut)) = cut {
            if let Some(request) = link.forget(cut) {
                let _ = net_service::served_gone(device, request);
            }
            if m != n {
                let _ = link.proto(|c| c.refuse(m, 400, "cut short by another request"));
            }
        }
        match device.served_head(server, method, target, Some(len), false, 0) {
            Ok(ServedDecision::Accept { request, path }) => {
                let open = Open {
                    link: number,
                    request,
                    path,
                    received: 0,
                };
                if link.keep(open).is_err() {
                    let _ = net_service::served_gone(device, request);
                    refuse_link(link, number, 503, "no memory for the request");
                } else if len == 0 && net_service::served_whole(device, request, 0).is_err() {
                    let _ = link.forget(number);
                    let _ = net_service::served_gone(device, request);
                    refuse_link(link, number, 503, "nothing serves the cable");
                }
            }
            Ok(ServedDecision::Reject(status)) => refuse_link(link, number, status, ""),
            Ok(ServedDecision::Busy) => refuse_link(link, number, 503, "busy"),
            Err(_) => refuse_link(link, number, 503, "nothing serves the cable"),
        }
    }

    /// Part of the body of the computer's request `n`: appended to its file,
    /// told to the service, and the request made whole with its last part.
    fn cable_data(&mut self, device: &NetDevice, n: u32, bytes: &[u8]) {
        let Some(link) = device.cable() else {
            return;
        };
        // The rest of a refused body is passed over, not answered a line at
        // a time (the answers to the computer stay bounded).
        if link.proto(|c| c.swallowed(n, bytes.len())) != Ok(false) {
            return;
        }
        let Ok(Some((number, last))) = link.proto(|c| c.data(n, bytes.len())) else {
            // Its request, if any, is cut short: its body goes.
            if let Ok(Some(number)) = link.proto(|c| c.cut(n))
                && let Some(request) = link.forget(number)
            {
                let _ = net_service::served_gone(device, request);
            }
            let _ = link.proto(|c| c.refuse(n, 400, "no such body coming"));
            return;
        };
        // The card the service receives bodies on (`.received/`).
        let storage = device.storage();
        let added = bytes.len() as u64;
        let written = link.with_open(number, |open| {
            let kept = storage.append(&open.path, bytes).is_ok();
            open.received = open.received.saturating_add(added);
            (open.request, open.received, kept)
        });
        let Some((request, received, kept)) = written else {
            return;
        };
        if !kept {
            let _ = link.forget(number);
            let _ = net_service::served_gone(device, request);
            refuse_link(link, number, 500, "the card could not keep it");
            return;
        }
        match net_service::served_body(device, request, received, added) {
            // The service ended it (past `max_body`, the card's reserve).
            Ok(Decision::Reject(status)) => {
                let _ = link.forget(number);
                refuse_link(link, number, status, "");
            }
            Ok(_) if last => {
                if net_service::served_whole(device, request, received).is_err() {
                    let _ = link.forget(number);
                    let _ = net_service::served_gone(device, request);
                    refuse_link(link, number, 503, "nothing serves the cable");
                }
            }
            Ok(_) => {}
            Err(_) => {
                let _ = link.forget(number);
                let _ = net_service::served_gone(device, request);
                refuse_link(link, number, 503, "nothing serves the cable");
            }
        }
    }

    /// The cable's server between callbacks: its start told, the requests
    /// answered reported gone, a held request delivered or answered 503.
    pub(crate) fn native_cable_pump(&mut self) {
        let Some(device) = self.native_cable() else {
            return;
        };
        let Some(link) = device.cable() else {
            return;
        };
        if let Some(server) = link.take_start()
            && net_service::server_started(&*device, server, "").is_err()
        {
            link.start_refused(server);
        }
        while let Some(request) = link.pop_gone() {
            let _ = net_service::served_gone(&*device, request);
        }
        if let Some(held) = link.take_held(self.now.millis())
            && let Some(server) = link.started()
        {
            self.cable_deliver(&device, server, held.n, &held.method, &held.target, 0);
        }
    }

    /// When the held request of the cable runs out of time.
    pub(crate) fn native_cable_due(&self) -> Option<u64> {
        self.native_cable()?.cable()?.held_until()
    }

    /// The next line of the native server's to send to the computer.
    pub(crate) fn native_cable_next_line(&self) -> Option<alloc::string::String> {
        self.cable_device()?.cable()?.next_line()
    }
}

#[cfg(test)]
#[path = "cable_pump_tests.rs"]
mod tests;
