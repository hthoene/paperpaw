//! Room made for a screen that needs memory ([`ScreenMode::memory`],
//! ADR 0030): as it shows, step by step until there is enough (caches
//! let go, background work held back, radios off where it does not need
//! them, the screens below asked to let go of what they make again, then
//! closed), and everything given back once it
//! closes: the radios as they were, the work going on, the screens opened
//! again, a book at its place. Nothing is said to the reader.
//!
//! [`ScreenMode::memory`]: paperpaw_sdk::realtime::ScreenMode::memory

use alloc::vec::Vec;

use paperpaw_sdk::builtin::events;
use paperpaw_sdk::memory::Placement;
use paperpaw_sdk::realtime::{Room, RoomSteps, Squeeze};
use paperpaw_sdk::wifi::{WifiCommand, WifiState};
use paperpaw_sdk::{Handled, Id, Params, ScreenKey};

use crate::Kernel;

/// Room made for a screen, and what to give back.
#[derive(Debug)]
pub(crate) struct Lease {
    /// The screen it is for.
    serial: u32,
    asked: u32,
    place: Option<Placement>,
    steps: RoomSteps,
    /// The screens closed below it, the lowest first, to open again.
    closed: Vec<(Id, Option<Id>, Params)>,
    /// Wi-Fi as it was: in a network (wanted again), or on alone.
    wifi: Option<WifiWas>,
    /// Bluetooth held off.
    bluetooth: bool,
    /// The rows of a frame's band before it was made smaller.
    rows: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WifiWas {
    Online,
    On,
}

impl Kernel {
    pub(crate) fn room_screen_keys_reserved(&self) -> usize {
        self.lease.as_ref().map_or(0, |lease| lease.closed.len())
    }

    /// The memory free where `place` says, where the shell can tell:
    /// internal RAM or PSRAM where there are both, else the largest block
    /// the heap could give (all it has free where it does not say).
    fn free_for(&self, place: Option<Placement>) -> Option<u32> {
        let used = self.shared.memory.read()?;
        let region = |external: bool| {
            used.regions
                .iter()
                .flatten()
                .filter(|r| r.external == external)
                .map(|r| r.free)
                .max()
        };
        let free = match place {
            Some(p) if p == Placement::FAST => region(false),
            Some(p) if p == Placement::LARGE => region(true),
            _ => None,
        };
        Some(free.unwrap_or_else(|| used.largest_free.unwrap_or(used.heap_free)))
    }

    /// As the screen on top shows: room made for it where it asks, given
    /// back where the screen that had it is gone.
    pub(crate) fn sync_room(&mut self) {
        if let Some(lease) = &self.lease
            && !self.stack.iter().any(|s| s.serial == lease.serial)
        {
            self.give_back();
        }
        let Some(top) = self.stack.last() else {
            return;
        };
        let serial = top.serial;
        let mode = top.screen.mode();
        let Some((asked, place)) = mode.memory_wanted() else {
            return;
        };
        if self.lease.as_ref().is_some_and(|l| l.serial == serial) {
            return;
        }
        // Another screen's room stays while it is below; this one asks
        // only once it is the one shown.
        if self.lease.is_some() {
            return;
        }
        self.lease = Some(Lease {
            serial,
            asked,
            place,
            steps: RoomSteps::default(),
            closed: Vec::new(),
            wifi: None,
            bluetooth: false,
            rows: None,
        });
        let enough = |k: &Kernel| k.free_for(place).is_none_or(|free| free >= asked);
        let steps: [fn(&mut Kernel) -> Option<RoomSteps>; 6] = [
            Kernel::room_from_caches,
            Kernel::room_from_work,
            Kernel::room_from_radios,
            Kernel::room_from_squeeze,
            Kernel::room_from_bands,
            Kernel::room_from_screens,
        ];
        for step in steps {
            if enough(self) {
                break;
            }
            if let Some(taken) = step(self)
                && let Some(lease) = &mut self.lease
            {
                lease.steps = lease.steps.with(taken);
            }
        }
        self.note_room();
    }

    /// Keeps what plugins read of the room up to date: radios switched off
    /// free their memory a moment later.
    pub(crate) fn note_room(&mut self) {
        self.shared.room = match &self.lease {
            Some(lease) => Room::new(lease.asked, self.free_for(lease.place), lease.steps),
            None => Room::default(),
        };
    }

    /// Glyphs drawn at runtime and what was read for the last frame.
    fn room_from_caches(&mut self) -> Option<RoomSteps> {
        self.shared.fonts.clear_cache();
        self.shared.recall.clear();
        Some(RoomSteps::CACHES)
    }

    /// Background work waits until the screen closes.
    fn room_from_work(&mut self) -> Option<RoomSteps> {
        self.works.hold(true);
        Some(RoomSteps::WORK)
    }

    /// Wi-Fi and Bluetooth off where the screen does not need them, and
    /// nothing goes over them now (a download is never cut).
    fn room_from_radios(&mut self) -> Option<RoomSteps> {
        let mode = self.stack.last()?.screen.mode();
        let mut taken = false;
        let busy = self.io.http.busy() || self.io.servers.busy() || self.io.sockets.any();
        if !mode.wants_network()
            && !busy
            && let Some(wifi) = &self.shared.wifi
        {
            let was = match wifi.state {
                // Hosting serves a phone: never taken away.
                WifiState::Off | WifiState::Hosting { .. } => None,
                WifiState::On => Some(WifiWas::On),
                _ => Some(WifiWas::Online),
            };
            if was.is_some() {
                self.io.wifi_calls.push(WifiCommand::Off);
                taken = true;
            }
            if let Some(lease) = &mut self.lease {
                lease.wifi = was;
            }
        }
        if !mode.wants_bluetooth()
            && let Some(radio) = &mut self.io.bluetooth
            && radio.is_on()
            && radio.hold_off(true, self.now)
        {
            taken = true;
            if let Some(lease) = &mut self.lease {
                lease.bluetooth = true;
            }
        }
        taken.then_some(RoomSteps::RADIOS)
    }

    /// The screens below let go of what they make again as they show (a
    /// book its pages, keeping its place), the lowest too
    /// ([`Screen::on_squeeze`](paperpaw_sdk::Screen::on_squeeze)).
    fn room_from_squeeze(&mut self) -> Option<RoomSteps> {
        let top = self.stack.len().checked_sub(1)?;
        let squeeze = Squeeze::new(self.lease.as_ref()?.asked);
        let mut taken = false;
        for index in 0..top {
            taken |= self.with_screen(index, |screen, cx| screen.on_squeeze(&squeeze, cx))
                == Handled::Yes;
        }
        taken.then_some(RoomSteps::SQUEEZED)
    }

    /// Frames drawn in bands of half the rows, where they are drawn in
    /// bands: each frame takes more passes.
    fn room_from_bands(&mut self) -> Option<RoomSteps> {
        let rows = self.framebuffer.rows();
        let align = self.frame.row_align.max(1);
        let half = (rows / 2 / align * align).max(align);
        if rows >= self.framebuffer.height() || half >= rows {
            return None;
        }
        self.framebuffer.set_rows(half).ok()?;
        if let Some(lease) = &mut self.lease {
            lease.rows = Some(rows);
        }
        Some(RoomSteps::BANDS)
    }

    /// The screens below closed, the lowest kept (something must be there
    /// once the screen closes): each keeps what it has as it closes, a
    /// book its place.
    fn room_from_screens(&mut self) -> Option<RoomSteps> {
        let top = self.stack.len().checked_sub(1)?;
        let below: Vec<(u32, Id, Option<Id>, Params)> = self.stack[1..top]
            .iter()
            .map(|s| (s.serial, s.id.clone(), s.slot.clone(), s.params.clone()))
            .collect();
        if below.is_empty() {
            return None;
        }
        // Normal opens must leave these keys available until give_back.
        if !self.require_screen_keys(below.len()) {
            return None;
        }
        for (serial, ..) in below.iter().rev() {
            self.close_screen(ScreenKey::new(*serial));
        }
        if let Some(lease) = &mut self.lease {
            lease.closed = below
                .into_iter()
                .map(|(_, id, slot, params)| (id, slot, params))
                .collect();
        }
        Some(RoomSteps::SCREENS)
    }

    /// Everything given back: the screens opened again (the lowest first),
    /// the radios as they were, the work going on.
    fn give_back(&mut self) {
        // Check before taking the lease or changing bands/radios. Normal
        // opens cannot spend its reserved keys, so this is a defensive refusal.
        if u32::try_from(self.room_screen_keys_reserved())
            .map_or(true, |count| count > u32::MAX - self.next_serial)
        {
            self.warn_runtime("room restoration refused: screen key space exhausted");
            return;
        }
        let Some(lease) = self.lease.take() else {
            return;
        };
        // Bands as they were, where there is room again (else smaller
        // ones go on).
        if let Some(rows) = lease.rows {
            let _ = self.framebuffer.set_rows(rows);
        }
        for (id, slot, params) in &lease.closed {
            self.open(id.clone(), slot.clone(), params);
        }
        match lease.wifi {
            Some(WifiWas::Online) => {
                self.output.commands.push(events::NETWORK_WANTED.emit(()));
                self.native_hint(crate::native_host::registrations::EVENT_NETWORK_WANTED_TAG);
            }
            Some(WifiWas::On) => self.io.wifi_calls.push(WifiCommand::On),
            None => {}
        }
        if lease.bluetooth
            && let Some(radio) = &mut self.io.bluetooth
        {
            radio.hold_off(false, self.now);
        }
        self.works.hold(false);
        self.note_room();
    }
}
