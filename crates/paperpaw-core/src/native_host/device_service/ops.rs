//! The `core.device` backend: decode the op, ask the [`DeviceHost`], encode the
//! reply exactly per schema into the scratch the dispatcher lends.
use super::{
    BluetoothFacts, BondedRow, BuiltinRow, DeviceFacts, DeviceHost, DeviceServices, FirmwareFacts,
    KEY, MAX_BONDED, MAX_KEYS, MAX_NETWORKS, MAX_NOT_INSTALLED, MAX_VALUES, NativeFacts,
    NetworkRow, NotInstalledRow, Reading, WifiFacts, caps,
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::registrations::{self as wire, Record};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};

/// Operation numbers of `core.device` major 1.
mod op {
    pub(super) const DEVICE: u32 = 1;
    pub(super) const VALUES: u32 = 2;
    pub(super) const FIRMWARE: u32 = 3;
    pub(super) const BUILTIN: u32 = 4;
    pub(super) const NATIVE: u32 = 5;
    pub(super) const CHECK: u32 = 6;
    pub(super) const MEMORY: u32 = 7;
    pub(super) const WIFI: u32 = 8;
    pub(super) const NETWORKS: u32 = 9;
    pub(super) const BLUETOOTH: u32 = 10;
    pub(super) const BONDED: u32 = 11;
    pub(super) const POWER: u32 = 12;
    pub(super) const INSTALL: u32 = 13;
    pub(super) const NOT_INSTALLED: u32 = 14;
    pub(super) const KEYS: u32 = 15;
}

/// The `core.device` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let services = frame
        .host_service(KEY)
        .and_then(<dyn core::any::Any>::downcast_ref::<DeviceServices>)
        .ok_or(ERR_UNSUPPORTED)?;
    let host: &dyn DeviceHost = &*services.0;
    match op.id {
        // Ops 6 `check` and 13 `install` read the request tables of the install
        // commands (`device.md` 3, 5.1), which another slice builds. Until
        // they exist these answer ERR_UNSUPPORTED on purpose, not by accident
        // of an unknown op.
        op::CHECK | op::INSTALL => Err(ERR_UNSUPPORTED),
        op::DEVICE | op::FIRMWARE | op::NATIVE | op::MEMORY | op::WIFI | op::BLUETOOTH => {
            no_input(input)?;
            single(host, op.id, out)
        }
        op::POWER => power(host, input, out),
        op::VALUES | op::KEYS => {
            no_input(input)?;
            plain_list(host, op, out)
        }
        op::BUILTIN | op::NETWORKS | op::BONDED | op::NOT_INSTALLED => {
            let query = Query::decode(host, input)?;
            paged_list(frame, host, op, &query, out)
        }
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// An op without input takes the empty batch (count 0).
fn no_input(input: &[u8]) -> Result<(), i32> {
    if input == [0] { Ok(()) } else { Err(ERR_INVALID) }
}

fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    wire::record_encode_one(record, out)
}

/// `text` cut at a UTF-8 boundary to at most `limit` bytes.
fn clip(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// The value `v` if it is at most `max`, else `fallback` (unknown enum values).
const fn known(v: u32, max: u32, fallback: u32) -> u32 {
    if v <= max { v } else { fallback }
}

/// Keep the first reply a host's visitor makes; later visits change nothing.
fn set(slot: &mut Option<Result<usize, i32>>, make: impl FnOnce() -> Result<usize, i32>) {
    if slot.is_none() {
        *slot = Some(make());
    }
}

/// The ops that answer one record.
fn single(host: &dyn DeviceHost, id: u32, out: &mut [u8]) -> Result<usize, i32> {
    let mut slot: Option<Result<usize, i32>> = None;
    match id {
        op::DEVICE => host.device(&mut |f| {
            set(&mut slot, || encode(&device_record(f), out));
        })?,
        op::FIRMWARE => host.firmware(&mut |f| {
            set(&mut slot, || encode(&firmware_record(f), out));
        })?,
        op::NATIVE => host.native(&mut |f| {
            set(&mut slot, || encode(&native_record(f), out));
        })?,
        op::WIFI => host.wifi(&mut |f| {
            set(&mut slot, || encode(&wifi_record(f), out));
        })?,
        op::MEMORY => return memory(host, out),
        op::BLUETOOTH => return bluetooth(host.bluetooth()?, out),
        _ => return Err(ERR_UNSUPPORTED),
    }
    // A host that returned Ok without a visit has nothing to say: not here.
    slot.unwrap_or(Err(ERR_NOT_FOUND))
}

fn device_record<'a>(f: &DeviceFacts<'a>) -> Record<'a> {
    Record::ServiceDevDevice(wire::ServiceDevDevice {
        id: clip(f.id, wire::SERVICE_DEV_DEVICE_ID_MAX_BYTES),
        model: clip(f.model, wire::SERVICE_DEV_DEVICE_MODEL_MAX_BYTES),
        catalog: clip(f.catalog, wire::SERVICE_DEV_DEVICE_CATALOG_MAX_BYTES),
        class: known(f.class, 2, 0),
        caps: f.caps & caps::CAPS_MASK,
        controls: f.controls & super::CONTROLS_MASK,
        tls_sessions: f.tls_sessions,
        start: known(f.start, 4, 0),
        slept_ms: f.slept_ms.unwrap_or(0),
        flags: u32::from(f.slept_ms.is_some()),
    })
}

fn firmware_record<'a>(f: &FirmwareFacts<'a>) -> Record<'a> {
    Record::ServiceDevFirmware(wire::ServiceDevFirmware {
        version: clip(f.version, wire::SERVICE_DEV_FIRMWARE_VERSION_MAX_BYTES),
        project: clip(f.project, wire::SERVICE_DEV_FIRMWARE_PROJECT_MAX_BYTES),
        chip: f.chip,
        room: f.room,
        undone: u32::from(f.undone),
        state: known(f.state, 4, 4),
        done: f.done,
        total: f.total,
        reason: f.reason,
        request: f.request,
    })
}

fn native_record<'a>(f: &NativeFacts<'a>) -> Record<'a> {
    Record::ServiceDevNative(wire::ServiceDevNative {
        abi_major: f.abi_major,
        abi_draft: f.abi_draft,
        target: f.target,
        code_file: clip(f.code_file, wire::SERVICE_DEV_NATIVE_CODE_FILE_MAX_BYTES),
        safe_mode: known(f.safe_mode, 2, 1),
        crashed: clip(f.crashed, wire::SERVICE_DEV_NATIVE_CRASHED_MAX_BYTES),
        last_loaded: clip(f.last_loaded, wire::SERVICE_DEV_NATIVE_LAST_LOADED_MAX_BYTES),
        pending: f.pending,
        not_installed: f.not_installed,
        room_free: f.room_free,
        room_largest: f.room_largest,
    })
}

fn wifi_record<'a>(f: &WifiFacts<'a>) -> Record<'a> {
    Record::ServiceDevWifi(wire::ServiceDevWifi {
        state: known(f.state, 4, 0),
        ssid: clip(f.ssid, wire::SERVICE_DEV_WIFI_SSID_MAX_BYTES),
        address: clip(f.address, wire::SERVICE_DEV_WIFI_ADDRESS_MAX_BYTES),
        host_name: clip(f.host_name, wire::SERVICE_DEV_WIFI_HOST_NAME_MAX_BYTES),
        guests: f.guests,
        flags: u32::from(f.scanning) | u32::from(f.wanted) << 1 | u32::from(f.busy) << 2,
    })
}

/// Op 7. Bit 0 cached, 1 `largest` known, 2 the stack values known, 3
/// external memory present; a field whose bit is clear is 0.
fn memory(host: &dyn DeviceHost, out: &mut [u8]) -> Result<usize, i32> {
    let m = host.memory()?;
    let (stack_free, stack_size) = m.stack.unwrap_or((0, 0));
    let (external_free, external_size) = m.external.unwrap_or((0, 0));
    encode(
        &Record::ServiceDevMemory(wire::ServiceDevMemory {
            heap_free: m.heap_free,
            heap_size: m.heap_size,
            largest: m.largest.unwrap_or(0),
            stack_free,
            stack_size,
            external_free,
            external_size,
            flags: u32::from(m.cached)
                | u32::from(m.largest.is_some()) << 1
                | u32::from(m.stack.is_some()) << 2
                | u32::from(m.external.is_some()) << 3,
        }),
        out,
    )
}

fn bluetooth(facts: BluetoothFacts, out: &mut [u8]) -> Result<usize, i32> {
    encode(
        &Record::ServiceDevBluetooth(wire::ServiceDevBluetooth {
            radio: known(facts.radio, 1, 0),
            max_links: facts.max_links,
            flags: 0,
        }),
        out,
    )
}

/// Op 12: `every_ms` is `dev_interval.base_ms` as interface 6's
/// [`Power::every`](paperpaw_sdk::power::Power::every) stretches it: the base in
/// Normal, times `stretch` in Saving (cut at `u32::MAX`), and 0 in Offline
/// ("never": a plugin reads `level` first) and for no base.
fn power(host: &dyn DeviceHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let Record::ServiceDevInterval(interval) = wire::records_decode_single(input)? else {
        return Err(ERR_INVALID);
    };
    let mut slot: Option<Result<usize, i32>> = None;
    host.power(&mut |power| {
        set(&mut slot, || {
            let every = match power.every(u64::from(interval.base_ms)) {
                Some(ms) if interval.base_ms != 0 => u32::try_from(ms).unwrap_or(u32::MAX),
                _ => 0,
            };
            let level = match power.level {
                paperpaw_sdk::power::PowerLevel::Saving => 1,
                paperpaw_sdk::power::PowerLevel::Offline => 2,
                _ => 0,
            };
            encode(
                &Record::ServiceDevPower(wire::ServiceDevPower {
                    level,
                    mode: clip(power.mode.as_str(), wire::SERVICE_DEV_POWER_MODE_MAX_BYTES),
                    flags: u32::from(power.automatic) | u32::from(power.online()) << 1,
                    stretch: power.stretch,
                    every_ms: every,
                }),
                out,
            )
        });
    })?;
    slot.unwrap_or(Err(ERR_NOT_FOUND))
}

/// A decoded, checked `reg_query` of a paged op.
struct Query {
    cursor: u32,
    /// A scope names nothing known here: an empty page, not an error.
    scoped: bool,
    generation: u32,
}

impl Query {
    /// The registry's paging rules (`registry.md` 2.1): a non-zero generation
    /// that is not the host's, or generation 0 with a non-zero cursor, is
    /// `ERR_STATE`; reserved `flags` are `ERR_INVALID`.
    fn decode(host: &dyn DeviceHost, input: &[u8]) -> Result<Self, i32> {
        let Record::ServiceRegQuery(query) = wire::records_decode_single(input)? else {
            return Err(ERR_INVALID);
        };
        if query.flags != 0 {
            return Err(ERR_INVALID);
        }
        let generation = host.generation().max(1);
        if query.generation != 0 && query.generation != generation
            || query.generation == 0 && query.cursor != 0
        {
            return Err(ERR_STATE);
        }
        Ok(Self {
            cursor: query.cursor,
            scoped: !query.scope.is_empty(),
            generation,
        })
    }
}

/// Collects the rows of a page and then the `end` record. A row is never cut.
struct Rows<'o> {
    out: &'o mut [u8],
    /// Where the rows stop: the `end` record's room is kept back.
    limit: usize,
    used: usize,
    from: u32,
    page_rows: u32,
    cap: u32,
    seen: u32,
    written: u32,
    full: bool,
    failure: Option<i32>,
}

impl<'o> Rows<'o> {
    fn new(out: &'o mut [u8], op: &ServiceOp, from: u32, cap: u32) -> Result<Self, i32> {
        let end = wire::ServiceEnd {
            flags: 0,
            total: 0,
            generation: 0,
            next: 0,
        };
        let reserved = wire::record_framed_len(&Record::ServiceEnd(end))?;
        let limit = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
        Ok(Self {
            out,
            limit,
            used: 0,
            from,
            page_rows: u32::from(op.reply_rows),
            cap,
            seen: 0,
            written: 0,
            full: false,
            failure: None,
        })
    }

    /// Row number `index` of the list, encoded when it is on this page.
    fn put<'r>(&mut self, index: u32, build: impl FnOnce() -> Record<'r>) {
        if self.failure.is_some()
            || self.full
            || index < self.from
            || self.written >= self.page_rows
        {
            return;
        }
        let Some(room) = self.out.get_mut(self.used..self.limit) else {
            self.failure = Some(ERR_NO_SPACE);
            return;
        };
        match encode(&build(), room) {
            Ok(length) => {
                self.used += length;
                self.written += 1;
            }
            Err(ERR_NO_SPACE) => self.full = true,
            Err(error) => self.failure = Some(error),
        }
    }

    /// The next row of a list in the host's order; stops at the list's cap.
    fn push<'r>(&mut self, build: impl FnOnce() -> Record<'r>) -> ControlFlow<()> {
        if self.seen >= self.cap || self.failure.is_some() {
            return ControlFlow::Break(());
        }
        let index = self.seen;
        self.seen += 1;
        self.put(index, build);
        ControlFlow::Continue(())
    }

    /// The `end` record. An unpaged list (`paged` false) carries only its
    /// total and fails with `ERR_NO_SPACE` rather than cut a row.
    fn finish(self, paged: bool, generation: u32) -> Result<usize, i32> {
        let total = self.seen;
        self.finish_total(paged, generation, total)
    }

    fn finish_total(self, paged: bool, generation: u32, total: u32) -> Result<usize, i32> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let next = self.from.min(total) + self.written;
        let end = if paged {
            wire::ServiceEnd {
                flags: u32::from(next >= total),
                total,
                generation,
                next,
            }
        } else if self.full || self.written != total {
            return Err(ERR_NO_SPACE);
        } else {
            wire::ServiceEnd {
                flags: 0,
                total,
                generation: 0,
                next: 0,
            }
        };
        let room = self.out.get_mut(self.used..).ok_or(ERR_NO_SPACE)?;
        Ok(self.used + encode(&Record::ServiceEnd(end), room)?)
    }
}

/// A name of op 2: `[a-z0-9-]{1,32}`.
fn value_name(name: &str) -> bool {
    (1..=wire::SERVICE_DEV_VALUE_NAME_MAX_BYTES).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Ops 2 and 15: lists that do not page. A host without them has none.
fn plain_list(host: &dyn DeviceHost, op: &ServiceOp, out: &mut [u8]) -> Result<usize, i32> {
    let cap = if op.id == op::VALUES { MAX_VALUES } else { MAX_KEYS };
    let mut rows = Rows::new(out, op, 0, cap)?;
    let status = if op.id == op::VALUES {
        host.values(&mut |reading: &Reading<'_>| {
            let Some(value) = reading.value.and_then(|v| i32::try_from(v).ok()) else {
                return ControlFlow::Continue(());
            };
            if !value_name(reading.name) {
                return ControlFlow::Continue(());
            }
            rows.push(|| {
                Record::ServiceDevValue(wire::ServiceDevValue {
                    name: reading.name,
                    value,
                })
            })
        })
    } else {
        host.keys(&mut |name| {
            if name.is_empty() {
                return ControlFlow::Continue(());
            }
            rows.push(|| {
                Record::ServiceDevKey(wire::ServiceDevKey {
                    name: clip(name, wire::SERVICE_DEV_KEY_NAME_MAX_BYTES),
                })
            })
        })
    };
    // Not known is an empty list for these two (`device.md` 2, ops 2 and 15).
    match status {
        Ok(()) | Err(ERR_NOT_FOUND) => rows.finish(false, 0),
        Err(error) => Err(error),
    }
}

/// Ops 4, 9, 11 and 14: lists paged like `core.registry`.
fn paged_list(
    frame: &dyn Frame,
    host: &dyn DeviceHost,
    op: &ServiceOp,
    query: &Query,
    out: &mut [u8],
) -> Result<usize, i32> {
    let cap = match op.id {
        op::NETWORKS => MAX_NETWORKS,
        op::BONDED => MAX_BONDED,
        op::NOT_INSTALLED => MAX_NOT_INSTALLED,
        _ => u32::MAX,
    };
    let mut rows = Rows::new(out, op, query.cursor, cap)?;
    let status = if query.scoped {
        // Only the empty scope names anything in DEV lists.
        Ok(())
    } else {
        match op.id {
            op::NETWORKS => return networks(host, query, rows),
            op::BUILTIN => builtin_rows(host, &mut rows),
            op::BONDED => bonded_rows(frame, host, &mut rows),
            _ => not_installed_rows(host, &mut rows),
        }
    };
    status?;
    rows.finish(true, query.generation)
}

fn builtin_rows(host: &dyn DeviceHost, rows: &mut Rows<'_>) -> Result<(), i32> {
    host.builtin(&mut |row: &BuiltinRow<'_>| {
        if row.id.is_empty() {
            return ControlFlow::Continue(());
        }
        rows.push(|| {
            Record::ServiceDevBuiltin(wire::ServiceDevBuiltin {
                id: clip(row.id, wire::SERVICE_DEV_BUILTIN_ID_MAX_BYTES),
                version: clip(row.version, wire::SERVICE_DEV_BUILTIN_VERSION_MAX_BYTES),
            })
        })
    })
}

/// Op 11. `given` is shown only to the plugin bound to the host slot
/// `core.bluetooth`; a frame that cannot name its caller shows it to nobody.
fn bonded_rows(frame: &dyn Frame, host: &dyn DeviceHost, rows: &mut Rows<'_>) -> Result<(), i32> {
    let manager = frame.caller().is_ok_and(|caller| host.bluetooth_manager(caller));
    host.bonded(&mut |row: &BondedRow<'_>| {
        rows.push(|| {
            Record::ServiceDevBonded(wire::ServiceDevBonded {
                device: row.device,
                name: clip(row.name, wire::SERVICE_DEV_BONDED_NAME_MAX_BYTES),
                kind: row.kind,
                flags: u32::from(row.connected) | u32::from(row.remote_keys) << 1,
                given: if manager {
                    clip(row.given, wire::SERVICE_DEV_BONDED_GIVEN_MAX_BYTES)
                } else {
                    ""
                },
            })
        })
    })
}

fn not_installed_rows(host: &dyn DeviceHost, rows: &mut Rows<'_>) -> Result<(), i32> {
    host.not_installed(&mut |row: &NotInstalledRow<'_>| {
        if row.plugin.is_empty() {
            return ControlFlow::Continue(());
        }
        rows.push(|| {
            Record::ServiceDevNotInstalled(wire::ServiceDevNotInstalled {
                plugin: clip(row.plugin, wire::SERVICE_DEV_NOT_INSTALLED_PLUGIN_MAX_BYTES),
                reason: if (1..=3).contains(&row.reason) {
                    row.reason
                } else {
                    2
                },
            })
        })
    })
}

/// The strongest [`MAX_NETWORKS`] networks, kept in order while the host's
/// visitor runs: (signal, the row's number among the usable rows), strongest
/// first, the earlier row first among equals. 256 bytes of stack, no heap.
struct Strongest {
    ranked: [(i32, u32); MAX_NETWORKS as usize],
    len: usize,
    /// Usable rows seen, also those that did not make the cut.
    usable: u32,
}

impl Strongest {
    fn new() -> Self {
        Self {
            ranked: [(0, 0); MAX_NETWORKS as usize],
            len: 0,
            usable: 0,
        }
    }

    fn add(&mut self, signal: i32) {
        let number = self.usable;
        self.usable += 1;
        let at = self
            .ranked
            .iter()
            .take(self.len)
            .take_while(|(held, _)| *held >= signal)
            .count();
        let mut carry = (signal, number);
        // Ripple the tail down by one; the weakest falls off at the cap.
        for slot in self.ranked.iter_mut().skip(at) {
            core::mem::swap(slot, &mut carry);
        }
        self.len = (self.len + 1).min(MAX_NETWORKS as usize);
    }

    fn number_of(&self, rank: u32) -> Option<u32> {
        self.ranked
            .get(..self.len)?
            .get(usize::try_from(rank).ok()?)
            .map(|(_, number)| *number)
    }
}

/// Op 9: strongest first, at most [`MAX_NETWORKS`]. Pass one ranks the host's
/// rows, then each row of the page is fetched by its number (a scan result
/// list is small, so one visit per row of a page is cheap and nothing is
/// copied or allocated). Rows without a name are left out.
fn networks(host: &dyn DeviceHost, query: &Query, mut rows: Rows<'_>) -> Result<usize, i32> {
    let mut best = Strongest::new();
    host.networks(&mut |row: &NetworkRow<'_>| {
        if !row.ssid.is_empty() {
            best.add(row.signal);
        }
        ControlFlow::Continue(())
    })?;
    let total = u32::try_from(best.len).map_err(|_| ERR_STATE)?;
    for rank in query.cursor..total.min(query.cursor.saturating_add(rows.page_rows)) {
        let wanted = best.number_of(rank).ok_or(ERR_STATE)?;
        let mut number = 0_u32;
        let mut found = false;
        host.networks(&mut |row: &NetworkRow<'_>| {
            if row.ssid.is_empty() {
                return ControlFlow::Continue(());
            }
            number += 1;
            if number - 1 != wanted {
                return ControlFlow::Continue(());
            }
            found = true;
            rows.put(rank, || {
                Record::ServiceDevNetwork(wire::ServiceDevNetwork {
                    ssid: clip(row.ssid, wire::SERVICE_DEV_NETWORK_SSID_MAX_BYTES),
                    signal: row.signal,
                    flags: u32::from(row.secured) | u32::from(row.known) << 1,
                })
            });
            ControlFlow::Break(())
        })?;
        // The host's list changed between the passes: start again.
        if !found {
            return Err(ERR_STATE);
        }
    }
    rows.finish_total(true, query.generation, total)
}
