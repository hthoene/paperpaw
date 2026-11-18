//! Plugins' Bluetooth commands: checked (whose the device is, who may),
//! carried out, and answered on the plugin's events, by their result or
//! by what failed.

use super::*;

use paperpaw_sdk::bluetooth::{BtCommand, Connect, Discover, Response};
use paperpaw_sdk::work::{Reason, Refusal, Retry};

/// What a command comes with: the screens open, whether it is the
/// reader's own doing, the time.
#[derive(Debug, Clone, Copy)]
pub struct Asked {
    pub screens: usize,
    pub reader: bool,
    pub now: Instant,
}

/// Which command `command` is, for its failure.
fn op_of(command: &BtCommand) -> BtOp {
    match command {
        BtCommand::Scan(_) | BtCommand::StopScan { .. } => BtOp::Scan,
        BtCommand::Connect(_) => BtOp::Connect,
        BtCommand::Disconnect { .. } => BtOp::Disconnect,
        BtCommand::Discover(_) => BtOp::Discover,
        BtCommand::Read { .. } => BtOp::Read,
        BtCommand::Write { .. } => BtOp::Write,
        BtCommand::Subscribe { .. } | BtCommand::SubscribeTo { .. } => BtOp::Subscribe,
        BtCommand::Unsubscribe { .. } => BtOp::Unsubscribe,
        BtCommand::Pair { .. } => BtOp::Pair,
        BtCommand::Forget { .. } => BtOp::Forget,
        BtCommand::Give { .. } | BtCommand::TakeBack { .. } => BtOp::Give,
        BtCommand::KeysOn { .. } | BtCommand::KeysOff { .. } => BtOp::Keys,
        BtCommand::HidReports { .. } => BtOp::HidReports,
        BtCommand::Advertise(_) | BtCommand::StopAdvertising => BtOp::Advertise,
        BtCommand::NotifyCentrals { .. } => BtOp::Notify,
        BtCommand::Respond { .. } | BtCommand::Refuse { .. } => BtOp::Respond,
        _ => BtOp::Extended,
    }
}

/// The device `command` is about, where it is about one.
fn device_of(command: &BtCommand) -> Option<DeviceId> {
    match command {
        BtCommand::Connect(connect) => Some(connect.device),
        BtCommand::Discover(discover) => Some(discover.device),
        BtCommand::Disconnect { device }
        | BtCommand::Read { device, .. }
        | BtCommand::Write { device, .. }
        | BtCommand::Subscribe { device, .. }
        | BtCommand::SubscribeTo { device, .. }
        | BtCommand::Unsubscribe { device, .. }
        | BtCommand::Pair { device }
        | BtCommand::Forget { device, .. }
        | BtCommand::Give { device, .. }
        | BtCommand::TakeBack { device, .. }
        | BtCommand::KeysOn { device, .. }
        | BtCommand::KeysOff { device, .. }
        | BtCommand::HidReports { device, .. } => Some(*device),
        _ => None,
    }
}

impl Radio {
    /// Where plugin `plugin`'s `command` is answered: the events it names,
    /// its link's, its service's.
    pub fn answer_to(&self, plugin: &str, command: &BtCommand) -> Option<Id> {
        match command {
            BtCommand::Scan(scan) => Some(scan.events.clone()),
            BtCommand::Connect(connect) => Some(connect.events.clone()),
            BtCommand::Advertise(advertising) => Some(advertising.events.clone()),
            BtCommand::StopScan { events }
            | BtCommand::Forget { events, .. }
            | BtCommand::Give { events, .. }
            | BtCommand::TakeBack { events, .. }
            | BtCommand::KeysOn { events, .. }
            | BtCommand::KeysOff { events, .. }
            | BtCommand::HidReports { events, .. } => Some(events.clone()),
            BtCommand::NotifyCentrals { service, .. } => self
                .served
                .iter()
                .find(|s| s.service == *service && s.events.namespace() == plugin)
                .map(|s| s.events.clone()),
            BtCommand::Respond { request, .. } | BtCommand::Refuse { request, .. } => self
                .requests
                .iter()
                .find(|(r, _)| *r == request.number())
                .map(|(_, e)| e.clone()),
            other => {
                let device = device_of(other)?;
                let link = self.links.iter().find(|l| l.device == device)?;
                link.events_of(plugin)
            }
        }
    }

    /// Plugin `plugin`'s command (the kernel checked it may use
    /// Bluetooth); what failed, where it did.
    pub fn command(
        &mut self,
        plugin: &str,
        command: BtCommand,
        asked: Asked,
        out: &mut Vec<Ask>,
    ) -> Result<(), BtFailure> {
        let op = op_of(&command);
        let device = device_of(&command);
        let result = self.carry_out(plugin, command, asked, out);
        self.settle(asked.now);
        result.map_err(|error| {
            let failure = BtFailure::new(op, error);
            match device {
                Some(device) => failure.device(device),
                None => failure,
            }
        })
    }

    fn carry_out(
        &mut self,
        plugin: &str,
        command: BtCommand,
        asked: Asked,
        out: &mut Vec<Ask>,
    ) -> Result<(), BtError> {
        match command {
            BtCommand::Scan(scan) => self.scan(plugin, scan, asked, out),
            BtCommand::StopScan { events } => {
                let at = self
                    .scans
                    .iter()
                    .position(|s| s.asker.events == events && s.asker.plugin == plugin)
                    .ok_or(BtError::NotFound)?;
                let scan = self.scans.remove(at);
                self.scan_ended(&scan.asker, asked.now, out);
                Ok(())
            }
            BtCommand::Connect(connect) => self.connect(plugin, connect, asked, out),
            BtCommand::Disconnect { device } => self.leave(plugin, device, asked.now, out),
            BtCommand::Discover(discover) => self.discover(plugin, discover, asked.now),
            BtCommand::Read { device, handle } => {
                let at = self.plugin_link(plugin, device)?;
                let link = &mut self.links[at];
                if let Some(events) = link.events_of(plugin) {
                    link.reads.push((Some(handle.get()), events));
                }
                link.active = asked.now;
                let number = link.number;
                self.calls.push(BtCall::Read {
                    link: number,
                    handle: handle.get(),
                });
                Ok(())
            }
            BtCommand::Write {
                device,
                handle,
                bytes,
                response,
            } => {
                let at = self.plugin_link(plugin, device)?;
                let link = &mut self.links[at];
                if let Some(events) = link.events_of(plugin) {
                    link.writes.push((handle.get(), events));
                }
                link.active = asked.now;
                let number = link.number;
                self.calls.push(BtCall::Write {
                    link: number,
                    handle: handle.get(),
                    bytes,
                    response: response == Response::Wanted,
                });
                Ok(())
            }
            BtCommand::Subscribe { device, handle } => {
                let at = self.plugin_link(plugin, device)?;
                self.subscribe(at, plugin, handle.get(), true);
                Ok(())
            }
            BtCommand::SubscribeTo {
                device,
                service,
                characteristic,
            } => self.subscribe_to(plugin, device, service, characteristic),
            BtCommand::Unsubscribe { device, handle } => {
                let at = self.plugin_link(plugin, device)?;
                self.subscribe(at, plugin, handle.get(), false);
                Ok(())
            }
            other => self.carry_out_more(plugin, other, asked, out),
        }
    }

    /// [`Radio::carry_out`] for pairing, bonds and keys.
    fn carry_out_more(
        &mut self,
        plugin: &str,
        command: BtCommand,
        asked: Asked,
        out: &mut Vec<Ask>,
    ) -> Result<(), BtError> {
        let may_manage = asked.reader || self.manager_is(plugin);
        match command {
            BtCommand::Pair { device } => {
                if !self.known.iter().any(|k| k.id == device && k.mine(plugin)) {
                    return Err(BtError::NotFound);
                }
                let at = self.plugin_link(plugin, device)?;
                let link = &mut self.links[at];
                link.pairing = link.events_of(plugin);
                let number = link.number;
                self.calls.push(BtCall::Pair { link: number });
                Ok(())
            }
            BtCommand::Forget { .. } | BtCommand::Give { .. } | BtCommand::TakeBack { .. }
                if !may_manage =>
            {
                Err(BtError::NotAllowed)
            }
            BtCommand::Forget { device, events } => {
                self.forget(device)?;
                out.push(tell(&events, BtEvent::done(BtOp::Forget, device)));
                Ok(())
            }
            BtCommand::Give {
                device,
                plugin: to,
                events,
            } => {
                self.give(device, to, true)?;
                out.push(tell(&events, BtEvent::done(BtOp::Give, device)));
                Ok(())
            }
            BtCommand::TakeBack {
                device,
                plugin: from,
                events,
            } => {
                self.give(device, from, false)?;
                out.push(tell(&events, BtEvent::done(BtOp::Give, device)));
                Ok(())
            }
            BtCommand::KeysOn { device, events } => {
                self.may_type(plugin, device, may_manage)?;
                self.keys(device, true, asked.now)?;
                out.push(tell(&events, BtEvent::done(BtOp::Keys, device)));
                Ok(())
            }
            BtCommand::KeysOff { device, events } => {
                self.may_type(plugin, device, may_manage)?;
                self.keys(device, false, asked.now)?;
                out.push(tell(&events, BtEvent::done(BtOp::Keys, device)));
                Ok(())
            }
            BtCommand::HidReports { device, events } => {
                let given = self
                    .known
                    .iter()
                    .any(|k| k.id == device && k.given_to(plugin));
                if !given {
                    return Err(BtError::NotFound);
                }
                let at = self.link_to(device, asked.now)?;
                let link = &mut self.links[at];
                link.raw.retain(|r| r.plugin != plugin);
                link.raw.push(Asker {
                    plugin: plugin.to_string(),
                    events,
                    screens: asked.screens,
                });
                if link.up {
                    let number = link.number;
                    self.calls.push(BtCall::Hid {
                        link: number,
                        on: true,
                    });
                }
                Ok(())
            }
            other => self.carry_out_served(plugin, other, asked, out),
        }
    }

    /// [`Radio::carry_out`] for being seen: advertising, the GATT server.
    fn carry_out_served(
        &mut self,
        plugin: &str,
        command: BtCommand,
        asked: Asked,
        out: &mut Vec<Ask>,
    ) -> Result<(), BtError> {
        match command {
            BtCommand::Advertise(mut advertising) => {
                // Fast only as the reader's own doing.
                if advertising.power == AdvertisePower::Fast && !asked.reader {
                    advertising = advertising.power(AdvertisePower::Balanced);
                }
                let asker = Asker {
                    plugin: plugin.to_string(),
                    events: advertising.events.clone(),
                    screens: asked.screens,
                };
                self.adverts.retain(|a| a.asker.plugin != plugin);
                let until = asked.now.add_millis(millis(advertising.lasting));
                out.push(tell(&asker.events, BtEvent::Advertising));
                self.adverts.push(AdvertRequest {
                    asker,
                    advertising,
                    until,
                });
                Ok(())
            }
            BtCommand::StopAdvertising => {
                let (ended, kept): (Vec<_>, Vec<_>) = self
                    .adverts
                    .drain(..)
                    .partition(|a| a.asker.plugin == plugin);
                self.adverts = kept;
                for advert in ended {
                    out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
                }
                Ok(())
            }
            BtCommand::NotifyCentrals {
                service,
                characteristic,
                bytes,
            } => {
                let served = self
                    .served
                    .iter()
                    .find(|s| {
                        s.service == service
                            && s.characteristic == characteristic
                            && s.events.namespace() == plugin
                    })
                    .ok_or(BtError::NotFound)?;
                let handle = served.handle;
                self.calls.push(BtCall::Notify { handle, bytes });
                Ok(())
            }
            BtCommand::Respond { request, bytes } => {
                self.take_request(plugin, request.number())?;
                self.calls.push(BtCall::Respond {
                    request: request.number(),
                    bytes,
                });
                Ok(())
            }
            BtCommand::Refuse { request, error } => {
                self.take_request(plugin, request.number())?;
                self.calls.push(BtCall::Refuse {
                    request: request.number(),
                    error: error.code(),
                });
                Ok(())
            }
            _ => Err(BtError::Unsupported),
        }
    }

    fn scan(
        &mut self,
        plugin: &str,
        mut scan: Scan,
        asked: Asked,
        out: &mut Vec<Ask>,
    ) -> Result<(), BtError> {
        // Not again so soon, unless the reader asked.
        let last = self
            .scanned
            .iter()
            .find(|(p, _)| p == plugin)
            .map(|(_, at)| *at);
        if let Some(last) = last
            && !asked.reader
            && asked.now.since(last) < SCAN_COOLDOWN_MS
        {
            let after = last.add_millis(SCAN_COOLDOWN_MS);
            let refusal = Refusal::new(Reason::TooOften, Retry::After(after));
            return Err(BtError::Held(refusal));
        }
        // Fast only as the reader's own doing.
        if scan.power == ScanPower::Fast && !asked.reader {
            scan = scan.power(ScanPower::Balanced);
        }
        let asker = Asker {
            plugin: plugin.to_string(),
            events: scan.events.clone(),
            screens: asked.screens,
        };
        self.scans.retain(|s| s.asker.events != scan.events);
        // What was found before, for this scan too.
        let mut told = false;
        for known in &mut self.known {
            let seen = known.found.signal.is_some();
            let others = known.bond.is_some() && !known.given_to(plugin);
            let device = known.device(plugin);
            if !seen || others || !scan.passes(&device) {
                continue;
            }
            if !known.found_by.iter().any(|p| p == plugin) {
                known.found_by.push(plugin.to_string());
            }
            out.push(tell(&asker.events, BtEvent::Found(device)));
            told = true;
            if scan.first {
                break;
            }
        }
        if scan.first && told {
            self.scan_ended(&asker, asked.now, out);
            return Ok(());
        }
        let until = asked.now.add_millis(millis(scan.lasting));
        self.scans.push(ScanRequest { asker, scan, until });
        Ok(())
    }

    fn connect(
        &mut self,
        plugin: &str,
        connect: Connect,
        asked: Asked,
        out: &mut Vec<Ask>,
    ) -> Result<(), BtError> {
        let device = connect.device;
        if !self.known.iter().any(|k| k.id == device && k.mine(plugin)) {
            return Err(BtError::NotFound);
        }
        let at = self.link_to(device, asked.now)?;
        let link = &mut self.links[at];
        link.users.retain(|u| u.asker.plugin != plugin);
        link.users.push(User {
            asker: Asker {
                plugin: plugin.to_string(),
                events: connect.events.clone(),
                screens: asked.screens,
            },
            power: connect.power,
            idle: connect.idle.map(millis),
        });
        if link.up {
            let event = BtEvent::connected(device, link.mtu, link.security);
            out.push(tell(&connect.events, event));
        }
        self.power_link(at, asked.now);
        Ok(())
    }

    fn discover(&mut self, plugin: &str, discover: Discover, now: Instant) -> Result<(), BtError> {
        let at = self.plugin_link(plugin, discover.device)?;
        let link = &mut self.links[at];
        if let Some(events) = link.events_of(plugin) {
            link.reads.push((None, events));
        }
        link.active = now;
        let number = link.number;
        self.calls.push(BtCall::Discover {
            link: number,
            services: discover.services,
        });
        Ok(())
    }

    /// Subscribes `plugin` to `handle` on link `at` (`on`), or no longer;
    /// the shell told where the first comes or the last goes.
    fn subscribe(&mut self, at: usize, plugin: &str, handle: u16, on: bool) {
        let link = &mut self.links[at];
        let Some(events) = link.events_of(plugin) else {
            return;
        };
        let before = link.subscribed.iter().any(|(h, _)| *h == handle);
        link.subscribed
            .retain(|(h, e)| !(*h == handle && *e == events));
        if on {
            link.subscribed.push((handle, events));
        }
        let after = link.subscribed.iter().any(|(h, _)| *h == handle);
        if before != after {
            let number = link.number;
            self.calls.push(BtCall::Subscribe {
                link: number,
                handle,
                on: after,
            });
        }
    }

    /// Subscribes to a characteristic by its UUIDs: at once where the
    /// services are known, else once found.
    fn subscribe_to(
        &mut self,
        plugin: &str,
        device: DeviceId,
        service: Uuid,
        characteristic: Uuid,
    ) -> Result<(), BtError> {
        let at = self.plugin_link(plugin, device)?;
        let link = &self.links[at];
        if let Some(found) =
            paperpaw_sdk::bluetooth::characteristic(&link.services, service, characteristic)
        {
            let handle = found.handle.get();
            self.subscribe(at, plugin, handle, true);
            return Ok(());
        }
        let link = &mut self.links[at];
        let events = link.events_of(plugin).ok_or(BtError::NotFound)?;
        link.pending.push((service, characteristic, events));
        let number = link.number;
        self.calls.push(BtCall::Discover {
            link: number,
            services: alloc::vec![service],
        });
        Ok(())
    }

    /// The plugin's link to `device`, where it is up and the plugin uses
    /// it.
    fn plugin_link(&self, plugin: &str, device: DeviceId) -> Result<usize, BtError> {
        self.links
            .iter()
            .position(|l| l.device == device && l.up && l.events_of(plugin).is_some())
            .ok_or(BtError::NotFound)
    }

    /// Plugin `plugin` lets go of its link to `device`.
    fn leave(
        &mut self,
        plugin: &str,
        device: DeviceId,
        now: Instant,
        out: &mut Vec<Ask>,
    ) -> Result<(), BtError> {
        let at = self
            .links
            .iter()
            .position(|l| l.device == device)
            .ok_or(BtError::NotFound)?;
        let link = &mut self.links[at];
        let (gone, kept): (Vec<_>, Vec<_>) =
            link.users.drain(..).partition(|u| u.asker.plugin == plugin);
        link.users = kept;
        link.raw.retain(|r| r.plugin != plugin);
        for user in &gone {
            let event = BtEvent::disconnected(device, None);
            out.push(tell(&user.asker.events, event));
        }
        self.keep_or_drop(at, now);
        if gone.is_empty() {
            Err(BtError::NotFound)
        } else {
            Ok(())
        }
    }

    fn forget(&mut self, device: DeviceId) -> Result<(), BtError> {
        let at = self
            .known
            .iter()
            .position(|k| k.id == device && k.bond.is_some())
            .ok_or(BtError::NotFound)?;
        let address = self.known[at].address.clone();
        self.known.remove(at);
        if let Some(link) = self.links.iter().position(|l| l.device == device) {
            self.release_keys(link);
            let link = self.links.remove(link);
            self.calls.push(BtCall::Disconnect { link: link.number });
        }
        self.calls.push(BtCall::Forget { address });
        self.calls.push(BtCall::Bonds(self.bonds()));
        self.bonds_changed = true;
        Ok(())
    }

    fn give(&mut self, device: DeviceId, plugin: Id, given: bool) -> Result<(), BtError> {
        let known = self
            .known_mut(device)
            .filter(|k| k.bond.is_some())
            .ok_or(BtError::NotFound)?;
        known.given.retain(|g| *g != plugin);
        if given {
            known.given.push(plugin);
        }
        self.bonds_changed = true;
        Ok(())
    }

    /// Whether `plugin` may switch the keys of `device`: the manager, the
    /// reader, or a plugin the device is given to.
    fn may_type(&self, plugin: &str, device: DeviceId, may_manage: bool) -> Result<(), BtError> {
        let known = self
            .known
            .iter()
            .find(|k| k.id == device && k.bond.is_some() && k.kind.can_type())
            .ok_or(BtError::NotFound)?;
        if may_manage || known.given_to(plugin) {
            Ok(())
        } else {
            Err(BtError::NotAllowed)
        }
    }

    /// The keys of bonded `device` go to the key maps (`on`), or no
    /// longer.
    pub(super) fn keys(&mut self, device: DeviceId, on: bool, now: Instant) -> Result<(), BtError> {
        let known = self.known_mut(device).ok_or(BtError::NotFound)?;
        known.wanted = on;
        if known.keys != on {
            known.keys = on;
            self.bonds_changed = true;
        }
        if on {
            let at = self.link_to(device, now)?;
            let link = &mut self.links[at];
            let start_hid = if link.keys.is_none() {
                link.keys = Some(now);
                link.up.then_some(link.number)
            } else {
                None
            };
            // As on connection, parameters must precede HID discovery.
            self.power_link(at, now);
            if let Some(number) = start_hid {
                self.calls.push(BtCall::Hid {
                    link: number,
                    on: true,
                });
            }
        } else if let Some(at) = self.links.iter().position(|l| l.device == device) {
            self.links[at].keys = None;
            self.release_keys(at);
            self.keep_or_drop(at, now);
        }
        Ok(())
    }

    fn take_request(&mut self, plugin: &str, request: u32) -> Result<(), BtError> {
        let at = self
            .requests
            .iter()
            .position(|(r, e)| *r == request && e.namespace() == plugin)
            .ok_or(BtError::NotFound)?;
        self.requests.remove(at);
        Ok(())
    }
}

/// A duration in milliseconds, as the kernel counts.
fn millis(duration: core::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
