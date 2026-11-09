//! The servers of `core.net` (`serve`) on the bridge: what core asks
//! ([`ShellHttp::listen`] and the rest) is queued or answered from what the
//! last step knew, the step opens and runs the connections (`serve7`), and
//! what happened goes to core as reports, core's decisions coming back as
//! verdicts at the next step. Nothing is reported from inside a transport
//! method, and nothing here allocates without a way to say no.

use alloc::vec::Vec;

use paperpaw_core::native_host::shell_wiring::{
    NetReports, ServedDecision, ServedReply, ServerReason,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::Instant;

use super::http::NetOut;
use super::{Bridge, Inner, Out, own, push_out};
use crate::net::Net;
use crate::serve7::{self, Event, Key, Reply, Verdict};

/// The heap a server needs free to open its sockets: two of 12 KiB and some
/// room besides.
const SERVER_HEAP: usize = 32 * 1024;

impl Inner {
    /// Opens and runs the servers a step on `net`; what happened into `out`.
    pub(super) fn serve_online(&mut self, net: &mut Net, now: Instant, out: &mut Vec<Out>) {
        self.address = net.address().map(|cidr| cidr.address());
        if self.servers.stale(net) {
            self.servers.lost(None);
            self.lost.raise_requests();
        }
        let mut events = Vec::new();
        while let Some(server) = self.unlistens.pop() {
            self.servers.stop(server, Some(&mut *net), &mut events);
        }
        while let Some((server, port)) = self.listens.pop() {
            if !self.servers.listen(server, port, net) {
                // Core was told the server started: the sockets it needed
                // could not be had after all (the heap was checked at
                // `listen`). Not told to core: it has no report for it.
                log::warn!("serve: server {server} could not open its sockets");
            }
        }
        while !self.replies.is_empty() {
            let reply = self.replies.remove(0);
            self.servers.reply(reply, now);
        }
        while let Some((request, status)) = self.closes7.pop() {
            self.servers.close_request(request, status, net, now);
        }
        if let Some(storage) = self.storage.clone() {
            self.servers
                .poll(net, storage.as_ref(), now, &mut self.verdicts, &mut events);
        }
        // A verdict not taken by its connection in this step is stale.
        self.verdicts.clear();
        for event in events {
            push_out(out, Out::Net(NetOut::Served(event)), &mut self.oom);
        }
    }

    /// No network: the servers end (core hears the network is lost).
    pub(super) fn serve_offline(&mut self, net: Option<&mut Net>) {
        let had = self.servers.any() || !self.listens.is_empty();
        self.servers.lost(net);
        self.listens.clear();
        self.unlistens.clear();
        self.replies.clear();
        self.closes7.clear();
        self.verdicts.clear();
        self.address = None;
        if had {
            self.lost.raise_requests();
        }
    }

    /// Keeps core's decision for `key` until the next step.
    fn put_verdict(&mut self, key: Key, verdict: Verdict) {
        if self.verdicts.try_reserve(1).is_ok() {
            self.verdicts.push((key, verdict));
        } else {
            self.oom = true;
        }
    }
}

impl Bridge {
    /// One report of a server event; `Err` with it where core was busy.
    pub(super) fn report_served(&self, net: &NetReports, event: Event) -> Result<(), Event> {
        match &event {
            Event::Head { server, conn, head } => {
                let length = (head.length > 0).then_some(head.length);
                let decision = net.served_head(
                    *server,
                    &head.method,
                    &head.target,
                    length,
                    head.chunked,
                    head.head_len,
                );
                let verdict = match decision {
                    Err(ERR_BUSY) => return Err(event),
                    Ok(ServedDecision::Accept { request, path }) => {
                        Verdict::Accept { request, path }
                    }
                    Ok(ServedDecision::Reject(status)) => Verdict::Reject(status),
                    Ok(_) => Verdict::Busy,
                    // The server stopped meanwhile: nothing to serve.
                    Err(_) => Verdict::Drop,
                };
                self.verdict(Key::Conn(*conn), verdict);
            }
            Event::Body {
                request,
                received,
                added,
            } => match net.served_body(*request, *received, *added) {
                Err(ERR_BUSY) => return Err(event),
                Ok(0) => {}
                Ok(status) => self.verdict(Key::Request(*request), Verdict::Reject(status)),
                Err(_) => self.verdict(Key::Request(*request), Verdict::Drop),
            },
            Event::Whole { request, total } => match net.served_whole(*request, *total) {
                Err(ERR_BUSY) => return Err(event),
                Ok(()) => {}
                Err(_) => self.verdict(Key::Request(*request), Verdict::Drop),
            },
            Event::Gone { request } => {
                if net.served_gone(*request) == Err(ERR_BUSY) {
                    return Err(event);
                }
            }
        }
        Ok(())
    }

    fn verdict(&self, key: Key, verdict: Verdict) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.put_verdict(key, verdict);
        }
    }

    /// `ShellHttp::listen`: the port and the address answered from what the
    /// last step knew, the sockets opened at the next step.
    pub(super) fn serve_listen(
        &self,
        server: u32,
        port: u32,
        address: &mut [u8],
    ) -> Result<usize, ServerReason> {
        let mut inner = self
            .inner
            .try_borrow_mut()
            .map_err(|_| ServerReason::Refused)?;
        let Some(ip) = inner.address else {
            return Err(ServerReason::NoNetwork);
        };
        // The cable server is not offered; a port is 1 to 65535.
        let want = u16::try_from(port)
            .ok()
            .filter(|p| *p != 0)
            .ok_or(ServerReason::Refused)?;
        if esp_alloc::HEAP.free() < SERVER_HEAP {
            return Err(ServerReason::Refused);
        }
        let mut taken = [0u16; 8];
        let mut count = 0;
        for (_, p) in inner.listens.iter().take(taken.len()) {
            taken[count] = *p;
            count += 1;
        }
        let chosen = inner
            .servers
            .free_port(want, &taken[..count])
            .ok_or(ServerReason::Refused)?;
        let text = serve7::address_text(ip, chosen);
        let bytes = text.as_bytes();
        let room = address
            .get_mut(..bytes.len())
            .ok_or(ServerReason::Refused)?;
        room.copy_from_slice(bytes);
        inner
            .listens
            .try_reserve(1)
            .map_err(|_| ServerReason::Refused)?;
        inner.listens.push((server, chosen));
        Ok(bytes.len())
    }

    /// `ShellHttp::unlisten`: the server stops at the next step.
    pub(super) fn serve_unlisten(&self, server: u32) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.listens.retain(|(s, _)| *s != server);
            if inner.unlistens.try_reserve(1).is_ok() {
                inner.unlistens.push(server);
            } else {
                inner.oom = true;
            }
        }
    }

    /// `ShellHttp::reply`: the answer, owned, sent by its connection at the
    /// next step.
    pub(super) fn serve_reply(&self, request: u32, reply: &ServedReply<'_>) -> Result<(), i32> {
        let mut headers = Vec::new();
        for (name, value) in reply.headers.clone() {
            headers.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            headers.push((own(name)?, own(value)?));
        }
        let mut body = Vec::new();
        if let Some(bytes) = reply.body {
            body.try_reserve_exact(bytes.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            body.extend_from_slice(bytes);
        }
        let file = match reply.send_path {
            Some(path) => Some((own(path)?, reply.file_len)),
            None => None,
        };
        let queued = Reply {
            request,
            status: u16::try_from(reply.status).unwrap_or(500),
            content_type: own(reply.content_type)?,
            headers,
            body,
            file,
        };
        let mut inner = self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        inner.replies.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        inner.replies.push(queued);
        Ok(())
    }

    /// `ShellHttp::close_served`.
    pub(super) fn serve_close(&self, request: u32, status: u32) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            if inner.closes7.try_reserve(1).is_ok() {
                inner.closes7.push((request, status));
            } else {
                inner.oom = true;
            }
        }
    }
}
