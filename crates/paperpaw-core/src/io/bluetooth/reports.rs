//! What the shell reports about Bluetooth, turned into the plugins'
//! events and into keys.

use super::*;

use paperpaw_hid::ReportMap;
use paperpaw_sdk::bluetooth::{GattRequest, Handle};

impl Radio {
    /// What the shell reported.
    pub fn report(&mut self, report: BtReport, now: Instant, out: &mut Vec<Command>) {
        match report {
            BtReport::Radio { on } => {
                if !on && self.radio_on {
                    // Off by itself: what used it ends, the remotes still
                    // wanted come back after a rest.
                    self.went_off(now, out);
                }
            }
            BtReport::Found(found) => self.found(found, now, out),
            BtReport::Connected {
                link,
                mtu,
                security,
            } => self.connected(link, mtu, security, now, out),
            BtReport::Disconnected { link, reason } => self.disconnected(link, reason, out),
            BtReport::Discovered { link, services } => self.discovered(link, services, out),
            BtReport::ServicesChanged { link } => {
                let Some(at) = self.link_at(link) else { return };
                let link = &mut self.links[at];
                link.services.clear();
                let device = link.device;
                for user in &link.users {
                    out.push(tell(&user.asker.events, BtEvent::services_changed(device)));
                }
            }
            BtReport::Value {
                link,
                handle,
                kind,
                bytes,
            } => self.value(link, handle, kind, bytes, now, out),
            BtReport::Written { link, handle } => self.written(link, handle, true, now, out),
            BtReport::Sent { link, handle } => self.written(link, handle, false, now, out),
            BtReport::Failed {
                link,
                op,
                handle,
                error,
            } => self.failed(link, op, handle, error, out),
            other => self.report_more(other, now, out),
        }
        self.settle(now);
    }

    /// [`Radio::report`] for pairing, HID and the GATT server.
    fn report_more(&mut self, report: BtReport, now: Instant, out: &mut Vec<Command>) {
        match report {
            BtReport::Ask { link, question } => {
                if let Some(at) = self.link_at(link) {
                    self.asking = Some((self.links[at].device, question));
                }
            }
            BtReport::Paired { link, keys } => self.paired(link, keys, out),
            BtReport::HidMap { link, map } => {
                let Some(at) = self.link_at(link) else { return };
                let device = self.links[at].device;
                for user in &self.links[at].raw {
                    let event = BtEvent::hid_report_map(device, map.clone());
                    out.push(tell(&user.events, event));
                }
                let decoder =
                    ReportMap::parse(&map).map_or_else(|_| Decoder::boot_keyboard(), Decoder::new);
                self.links[at].decoder = Some(decoder);
            }
            BtReport::HidReport {
                link,
                report,
                bytes,
            } => self.hid_report(link, report, &bytes, now, out),
            BtReport::ReadRequest {
                request,
                handle,
                central,
            } => {
                let central = self.device_at(&central);
                let Some(served) = self.served.iter().find(|s| s.handle == handle).cloned() else {
                    return;
                };
                self.requests.push((request, served.events.clone()));
                let event = BtEvent::read_request(
                    GattRequest::new(request),
                    served.service,
                    served.characteristic,
                    central,
                );
                out.push(tell(&served.events, event));
            }
            BtReport::WriteRequest {
                request,
                handle,
                central,
                bytes,
            } => {
                let central = self.device_at(&central);
                let Some(served) = self.served.iter().find(|s| s.handle == handle).cloned() else {
                    return;
                };
                if let Some(request) = request {
                    self.requests.push((request, served.events.clone()));
                }
                let event = BtEvent::write_request(
                    request.map(GattRequest::new),
                    served.service,
                    served.characteristic,
                    central,
                    bytes,
                );
                out.push(tell(&served.events, event));
            }
            BtReport::Subscribed {
                handle,
                central,
                on,
            } => {
                let central = self.device_at(&central);
                if let Some(served) = self.served.iter().find(|s| s.handle == handle) {
                    let (service, characteristic) = (served.service, served.characteristic);
                    let event = if on {
                        BtEvent::subscribed(service, characteristic, central)
                    } else {
                        BtEvent::unsubscribed(service, characteristic, central)
                    };
                    out.push(tell(&served.events, event));
                }
            }
            BtReport::Advertising { on: false } => {
                for advert in self.adverts.drain(..) {
                    out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
                }
                self.advertised = None;
            }
            _ => {}
        }
    }

    /// A device a scan found: told to every scan it passes, of plugins
    /// that may see it.
    fn found(&mut self, found: Found, now: Instant, out: &mut Vec<Command>) {
        let id = self.device_at(&found.address);
        let Some(at) = self.known.iter().position(|k| k.id == id) else {
            return;
        };
        let known = &mut self.known[at];
        let first = known.found.signal.is_none();
        let changed = first
            || (found.name.is_some() && known.found.name != found.name)
            || found
                .services
                .iter()
                .any(|s| !known.found.services.contains(s))
            || known.found.manufacturer_data != found.manufacturer_data
            || known.found.service_data != found.service_data;
        let name = found.name.clone().or_else(|| known.found.name.clone());
        let mut services = known.found.services.clone();
        for service in &found.services {
            if !services.contains(service) {
                services.push(*service);
            }
        }
        known.found = Found {
            name,
            services,
            signal: found.signal.or(known.found.signal).or(Some(-100)),
            ..found
        };
        if known.kind == DeviceKind::Other || known.found.appearance.is_some() {
            known.kind = kind_of(known.found.appearance, &known.found.services);
        }
        // A bonded device whose keys are wanted is back: connected at once.
        let awaited = known.wanted && known.bond.is_some();
        if awaited && !self.links.iter().any(|l| l.device == id) {
            let _ = self.keys(id, true, now);
        }
        if !changed {
            return;
        }
        let mut ended = Vec::new();
        for (index, scan) in self.scans.iter().enumerate() {
            let plugin = scan.asker.plugin.as_str();
            let known = &mut self.known[at];
            // Someone's bond only for a plugin it is given to.
            if known.bond.is_some() && !known.given_to(plugin) {
                continue;
            }
            if !first && !scan.scan.updates {
                continue;
            }
            let device = known.device(plugin);
            if !scan.scan.passes(&device) {
                continue;
            }
            if !known.found_by.iter().any(|p| p == plugin) {
                known.found_by.push(plugin.to_string());
            }
            out.push(tell(&scan.asker.events, BtEvent::Found(device)));
            if scan.scan.first {
                ended.push(index);
            }
        }
        for index in ended.into_iter().rev() {
            let scan = self.scans.remove(index);
            self.scan_ended(&scan.asker, now, out);
        }
    }

    fn connected(
        &mut self,
        link: u32,
        mtu: u16,
        security: Security,
        now: Instant,
        out: &mut Vec<Command>,
    ) {
        let Some(at) = self.link_at(link) else { return };
        let link = &mut self.links[at];
        link.up = true;
        link.mtu = mtu;
        link.security = security;
        link.active = now;
        let device = link.device;
        for user in &link.users {
            let event = BtEvent::connected(device, mtu, security);
            out.push(tell(&user.asker.events, event));
        }
        let (number, hid, power) = (
            link.number,
            link.keys.is_some() || !link.raw.is_empty(),
            link.power,
        );
        // The shell handles calls in order. Apply the link's requested
        // parameters before HID discovery can wait on many ATT replies.
        if power != LinkPower::Low {
            self.calls.push(BtCall::LinkPower {
                link: number,
                power,
            });
        }
        if hid {
            self.calls.push(BtCall::Hid {
                link: number,
                on: true,
            });
        }
    }

    fn disconnected(&mut self, link: u32, reason: Option<BtError>, out: &mut Vec<Command>) {
        let Some(at) = self.link_at(link) else { return };
        self.release_keys(at);
        let link = self.links.remove(at);
        let asked = self.asking.is_some_and(|(d, _)| d == link.device);
        if link.pairing.is_some() || asked {
            self.pairing_over(link.device, true);
        }
        let reason = reason.or(Some(BtError::Lost));
        let mut told: Vec<Id> = link.users.iter().map(|u| u.asker.events.clone()).collect();
        told.extend(link.raw.iter().map(|r| r.events.clone()));
        told.extend(link.pairing.iter().cloned());
        paperpaw_sort::sort_by(&mut told, |a, b| a.as_str().cmp(b.as_str()));
        told.dedup();
        for events in told {
            out.push(tell(&events, BtEvent::disconnected(link.device, reason)));
        }
    }

    fn discovered(&mut self, link: u32, services: Vec<GattService>, out: &mut Vec<Command>) {
        let Some(at) = self.link_at(link) else { return };
        let device = self.links[at].device;
        // Merged with what was found before (a discovery of one service).
        for service in services.iter().cloned() {
            let known = &mut self.links[at].services;
            known.retain(|s| s.uuid != service.uuid);
            known.push(service);
        }
        let link = &mut self.links[at];
        let (waiting, kept): (Vec<_>, Vec<_>) =
            link.reads.drain(..).partition(|(h, _)| h.is_none());
        link.reads = kept;
        for (_, events) in waiting {
            out.push(tell(&events, BtEvent::discovered(device, services.clone())));
        }
        // Subscriptions by UUID, now that the handles are known.
        let pending = core::mem::take(&mut self.links[at].pending);
        for (service, characteristic, events) in pending {
            let found = paperpaw_sdk::bluetooth::characteristic(
                &self.links[at].services,
                service,
                characteristic,
            )
            .map(|c| c.handle.get());
            match found {
                Some(handle) => {
                    let plugin = events.namespace().to_string();
                    self.subscribe_events(at, &plugin, &events, handle);
                }
                None => {
                    let failure = BtFailure::new(BtOp::Subscribe, BtError::NotFound).device(device);
                    out.push(fail(&events, failure));
                }
            }
        }
    }

    /// `events` subscribed to `handle` on link `at`; the shell told where
    /// it is the first.
    fn subscribe_events(&mut self, at: usize, _plugin: &str, events: &Id, handle: u16) {
        let link = &mut self.links[at];
        let first = !link.subscribed.iter().any(|(h, _)| *h == handle);
        link.subscribed
            .retain(|(h, e)| !(*h == handle && e == events));
        link.subscribed.push((handle, events.clone()));
        if first {
            let number = link.number;
            self.calls.push(BtCall::Subscribe {
                link: number,
                handle,
                on: true,
            });
        }
    }

    fn value(
        &mut self,
        link: u32,
        handle: u16,
        kind: ValueKind,
        bytes: Vec<u8>,
        now: Instant,
        out: &mut Vec<Command>,
    ) {
        let Some(at) = self.link_at(link) else { return };
        let link = &mut self.links[at];
        link.active = now;
        let device = link.device;
        // A read's answer to who read, else what the device sent to who
        // subscribed.
        let to: Vec<Id> = match link.reads.iter().position(|(h, _)| *h == Some(handle)) {
            Some(i) if kind == ValueKind::Read => alloc::vec![link.reads.remove(i).1],
            _ => link
                .subscribed
                .iter()
                .filter(|(h, _)| *h == handle)
                .map(|(_, e)| e.clone())
                .collect(),
        };
        for events in to {
            let event = BtEvent::value(device, Handle::new(handle), kind, bytes.clone());
            out.push(tell(&events, event));
        }
    }

    /// A write was done (`answered`), or one without response went out.
    fn written(
        &mut self,
        link: u32,
        handle: u16,
        answered: bool,
        now: Instant,
        out: &mut Vec<Command>,
    ) {
        let Some(at) = self.link_at(link) else { return };
        let link = &mut self.links[at];
        link.active = now;
        let device = link.device;
        if let Some(i) = link.writes.iter().position(|(h, _)| *h == handle) {
            let (_, events) = link.writes.remove(i);
            let handle = Handle::new(handle);
            let event = if answered {
                BtEvent::written(device, handle)
            } else {
                BtEvent::sent(device, handle)
            };
            out.push(tell(&events, event));
        }
    }

    /// What was asked on a link failed: told to who asked (a read, a
    /// write, a discovery, pairing), else to every user.
    fn failed(
        &mut self,
        link: u32,
        op: BtOp,
        handle: Option<u16>,
        error: BtError,
        out: &mut Vec<Command>,
    ) {
        let Some(at) = self.link_at(link) else { return };
        let device = self.links[at].device;
        let l = &mut self.links[at];
        let asked: Option<Id> = match (op, handle) {
            (BtOp::Read, Some(h)) => take_first(&mut l.reads, |(r, _)| *r == Some(h)).map(|r| r.1),
            (BtOp::Write, Some(h)) => take_first(&mut l.writes, |(w, _)| *w == h).map(|w| w.1),
            (BtOp::Discover, _) => take_first(&mut l.reads, |(r, _)| r.is_none()).map(|r| r.1),
            (BtOp::Pair, _) => l.pairing.take(),
            _ => None,
        };
        let told: Vec<Id> = match asked {
            Some(events) => alloc::vec![events],
            None => l.users.iter().map(|u| u.asker.events.clone()).collect(),
        };
        let mut failure = BtFailure::new(op, error).device(device);
        if let Some(handle) = handle {
            failure = failure.handle(Handle::new(handle));
        }
        for events in told {
            out.push(fail(&events, failure));
        }
        if op == BtOp::Pair {
            self.pairing_over(device, false);
        }
        if !self.links[at].up {
            // Never connected: the link is gone.
            self.links.remove(at);
        }
    }

    fn paired(&mut self, link: u32, keys: String, out: &mut Vec<Command>) {
        let Some(at) = self.link_at(link) else { return };
        let device = self.links[at].device;
        let events = self.links[at].pairing.take();
        self.pairing_over(device, false);
        if let Some(known) = self.known_mut(device) {
            known.bond = Some(keys);
            if let Some(events) = &events
                && let Ok(plugin) = Id::parse(events.namespace())
                && !known.given.contains(&plugin)
            {
                known.given.push(plugin);
            }
        }
        self.bonds_changed = true;
        self.calls.push(BtCall::Bonds(self.bonds()));
        if let Some(events) = events {
            out.push(tell(&events, BtEvent::paired(device)));
        }
    }

    fn hid_report(
        &mut self,
        link: u32,
        report: Option<u8>,
        bytes: &[u8],
        now: Instant,
        out: &mut Vec<Command>,
    ) {
        let Some(at) = self.link_at(link) else { return };
        let device = self.links[at].device;
        for user in &self.links[at].raw {
            let event = BtEvent::hid_report(device, report, bytes.to_vec());
            out.push(tell(&user.events, event));
        }
        if self.links[at].keys.is_some() {
            self.links[at].keys = Some(now);
            let decoder = self.links[at]
                .decoder
                .get_or_insert_with(Decoder::boot_keyboard);
            for change in decoder.report(report, bytes) {
                self.keys_out.push((change.key.name(), change.pressed));
            }
            // A key came: responsive again.
            self.power_link(at, now);
        }
    }
}

/// The first item of `list` where `which` holds, taken out.
fn take_first<T>(list: &mut Vec<T>, which: impl Fn(&T) -> bool) -> Option<T> {
    let at = list.iter().position(which)?;
    Some(list.remove(at))
}
