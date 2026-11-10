//! The `core.device` backend: decode the op, ask the [`DeviceHost`], encode the
//! reply exactly per schema into the scratch the dispatcher lends.
use super::{
    BluetoothFacts, BondedRow, BuiltinRow, DeviceFacts, DeviceHost, FirmwareFacts, MAX_BONDED,
    MAX_KEYS, MAX_NETWORKS, MAX_NOT_INSTALLED, MAX_VALUES, NativeFacts, NetworkRow,
    NotInstalledRow, Reading, WifiFacts, caps,
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use crate::native_host::host_services::Caller;
use crate::native_host::registrations::{self as wire, Record};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, OK,
};
use paperpaw_platform::firmware::Signer;

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
    pub(super) const LEAVES_OUT: u32 = 16;
}

/// The `core.device` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let services = who.hosts.device().ok_or(ERR_UNSUPPORTED)?;
    // The kernel answers Wi-Fi, Bluetooth and power from its own models, where
    // the frame has them (`kernel_models`).
    let kernel;
    let host: &dyn DeviceHost = match frame.kernel_env() {
        Some(env) => {
            kernel = super::kernel_models::KernelHost {
                inner: &*services.0,
                env,
                caller: who.plugin,
            };
            &kernel
        }
        None => &*services.0,
    };
    match op.id {
        // Op 13 `install` reads the firmware request table (a firmware
        // install or check); plugin installs and removals have no table yet.
        // Ops 3, 6 and 16 read the firmware install machine.
        op::INSTALL => install(who, input, out),
        op::CHECK => check(who, input, out),
        op::LEAVES_OUT => leaves_out(who, host, op, input, out),
        op::FIRMWARE => {
            no_input(input)?;
            firmware(who, host, out)
        }
        op::DEVICE | op::NATIVE | op::MEMORY | op::WIFI | op::BLUETOOTH => {
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
            paged_list(who.owner, host, op, &query, out)
        }
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// An op without input takes the empty batch (count 0).
fn no_input(input: &[u8]) -> Result<(), i32> {
    if input == [0] {
        Ok(())
    } else {
        Err(ERR_INVALID)
    }
}

fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    codec::encode_one(record, out)
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
        // The dual-boot facts (bit 0 shared, bit 1 ini_off) come with the
        // dual-boot host side (Core todo 397); until then none are told,
        // as an older host's record reads.
        flags: 0,
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
        last_loaded: clip(
            f.last_loaded,
            wire::SERVICE_DEV_NATIVE_LAST_LOADED_MAX_BYTES,
        ),
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
    let interval: wire::ServiceDevInterval = codec::single(input)?;
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
        let query: wire::ServiceRegQuery<'_> = codec::single(input)?;
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
            cursor: None,
        };
        let reserved = codec::framed_len(&Record::ServiceEnd(end))?;
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
                cursor: None,
            }
        } else if self.full || self.written != total {
            return Err(ERR_NO_SPACE);
        } else {
            wire::ServiceEnd {
                flags: 0,
                total,
                generation: 0,
                next: 0,
                cursor: None,
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
    let cap = if op.id == op::VALUES {
        MAX_VALUES
    } else {
        MAX_KEYS
    };
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
    owner: u64,
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
            op::BONDED => bonded_rows(owner, host, &mut rows),
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
/// `core.bluetooth`.
fn bonded_rows(owner: u64, host: &dyn DeviceHost, rows: &mut Rows<'_>) -> Result<(), i32> {
    let manager = host.bluetooth_manager(owner);
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

/// The running built-ins as op 4 lists them: its rows in its order, a row
/// without an id left out. The firmware check numbers the image's built-ins
/// with this and op 16 lists the rows by the same numbers, so a bit never
/// points at another row (`device.md` 5.1).
#[allow(
    dead_code,
    reason = "the kernel's firmware pump numbers built-ins with it (slice B2); only tests build it until then"
)]
pub(crate) struct DeviceBuiltIns<'a>(pub(crate) &'a dyn DeviceHost);

impl crate::native_host::firmware_install::BuiltIns for DeviceBuiltIns<'_> {
    fn count(&self) -> usize {
        let mut count = 0_usize;
        let _ = self.0.builtin(&mut |row: &BuiltinRow<'_>| {
            if !row.id.is_empty() {
                count += 1;
            }
            ControlFlow::Continue(())
        });
        count
    }

    fn index_of(&self, plugin_id: &str) -> Option<usize> {
        let (mut index, mut found) = (0_usize, None);
        let _ = self.0.builtin(&mut |row: &BuiltinRow<'_>| {
            if row.id.is_empty() {
                return ControlFlow::Continue(());
            }
            if row.id == plugin_id {
                found = Some(index);
                return ControlFlow::Break(());
            }
            index += 1;
            ControlFlow::Continue(())
        });
        found
    }
}

/// Op 3: the host's facts, with the install state, progress and last outcome
/// of the install machine where there is one (the host leaves them idle).
fn firmware(who: &Caller<'_>, host: &dyn DeviceHost, out: &mut [u8]) -> Result<usize, i32> {
    let mut slot: Option<Result<usize, i32>> = None;
    host.firmware(&mut |facts| {
        let mut facts = *facts;
        if let Some(machine) = who.hosts.firmware_install() {
            // A machine in use (borrowed) is a host fault: not idle by guess.
            match machine.try_borrow() {
                Ok(machine) => {
                    facts.state = machine.state().number();
                    (facts.done, facts.total) = machine.progress();
                    let (request, reason) = machine.last(who.owner);
                    facts.request = request;
                    facts.reason = u32::from(reason);
                }
                Err(_) => set(&mut slot, || Err(ERR_BUSY)),
            }
        }
        set(&mut slot, || encode(&firmware_record(&facts), out));
    })?;
    slot.unwrap_or(Err(ERR_NOT_FOUND))
}

/// `dev_check.flags` bits of a firmware check (`device.md` 5.1).
const CHECK_SIGNED_BY_PAPERPAW: u32 = 1 << 3;
const CHECK_FOREIGN: u32 = 1 << 4;
const CHECK_LEAVES_OUT: u32 = 1 << 5;
/// Bits 0 to 2 of a plugin check's flags.
const CHECK_PLUGIN_BITS: u32 = 0b111;

/// Op 13: the state of the caller's request, and the outcome kept for one
/// that ended (until its next request of the same kind or its stop).
fn install(who: &Caller<'_>, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let table = who.hosts.firmware_requests().ok_or(ERR_UNSUPPORTED)?;
    let asked: wire::ServiceDevRequest = codec::single(input)?;
    let found = table
        .try_borrow()
        .map_err(|_| ERR_BUSY)?
        .lookup(who.owner, asked.request)
        .ok_or(ERR_NOT_FOUND)?;
    let ended = found.outcome;
    encode(
        &Record::ServiceDevInstall(wire::ServiceDevInstall {
            request: asked.request,
            kind: found.kind,
            state: found.state,
            status: ended.map_or(OK, |outcome| {
                crate::native_host::firmware_events::status_of(outcome)
            }),
            reason: ended.map_or(0, |outcome| u32::from(outcome.reason)),
            flags: 0,
            done: found.done,
            total: found.total,
            id: "",
        }),
        out,
    )
}

/// Op 6: the caller's last finished firmware check, by its request.
fn check(who: &Caller<'_>, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let machine = who.hosts.firmware_install().ok_or(ERR_UNSUPPORTED)?;
    let asked: wire::ServiceDevRequest = codec::single(input)?;
    let machine = machine.try_borrow().map_err(|_| ERR_BUSY)?;
    let checked = machine.checked(who.owner)?;
    if checked.request != asked.request {
        return Err(ERR_NOT_FOUND);
    }
    let (mut signer, mut key, mut flags) = ("", [0_u8; 32], 0_u32);
    let mut keyed = false;
    let (mut bytes, mut id, mut version) = (0_u64, "", "");
    let mut digest = [0_u8; 32];
    if let Some(facts) = checked.facts() {
        bytes = u64::from(facts.size);
        id = facts.project.as_str();
        version = facts.version.as_str();
        match &facts.signer {
            Signer::PaperPaw { name, key: found } => {
                (signer, key, keyed) = (name.as_str(), *found, true);
                flags |= CHECK_SIGNED_BY_PAPERPAW;
            }
            Signer::Other { key: found } => (key, keyed) = (*found, true),
            _ => {}
        }
        if facts.foreign {
            flags |= CHECK_FOREIGN;
        }
        // The digest is told for a signed image only.
        digest = facts.sha256;
    } else if let Some(facts) = checked.plugin_facts() {
        (bytes, id, version) = (facts.bytes, facts.id.as_str(), facts.version.as_str());
        // Bits 0 to 2 are the job's (data plugin, pinned here, pinned to
        // another key); the key's name is one the device knows, PaperPaw's.
        flags = facts.flags & CHECK_PLUGIN_BITS;
        signer = facts.signer.as_str();
        if !signer.is_empty() {
            flags |= CHECK_SIGNED_BY_PAPERPAW;
        }
        // The key and the code's digest come together, absent when unsigned.
        if let (Some(found), Some(sum)) = (facts.key, facts.sha256) {
            (key, digest, keyed) = (found, sum, true);
        }
    }
    if checked.leaves_out_some() {
        flags |= CHECK_LEAVES_OUT;
    }
    encode(
        &Record::ServiceDevCheck(wire::ServiceDevCheck {
            reason: u32::from(checked.reason),
            needs_draft: checked.needs_draft(),
            bytes,
            signer: clip(signer, wire::SERVICE_DEV_CHECK_SIGNER_MAX_BYTES),
            key: if keyed { &key } else { &[] },
            id: clip(id, wire::SERVICE_DEV_CHECK_ID_MAX_BYTES),
            version: clip(version, wire::SERVICE_DEV_CHECK_VERSION_MAX_BYTES),
            flags,
            sha256: if keyed { &digest } else { &[] },
        }),
        out,
    )
}

/// Op 16: the running built-ins (op 4's rows, in its order) the caller's last
/// finished firmware check's image does not have, paged. The `end`'s
/// generation is the check's request: a cursor from another check is
/// `ERR_STATE`.
fn leaves_out(
    who: &Caller<'_>,
    host: &dyn DeviceHost,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let machine = who.hosts.firmware_install().ok_or(ERR_UNSUPPORTED)?;
    let query: wire::ServiceRegQuery<'_> = codec::single(input)?;
    if query.flags != 0 || !query.scope.is_empty() {
        return Err(ERR_INVALID);
    }
    let machine = machine.try_borrow().map_err(|_| ERR_BUSY)?;
    let checked = machine.leaves_out_of(who.owner)?;
    // Never 0: 0 asks for the first page (a request number is the sender's own
    // and non-zero, but the paging does not depend on it).
    let generation = checked.request.max(1);
    if query.generation != 0 && query.generation != generation
        || query.generation == 0 && query.cursor != 0
    {
        return Err(ERR_STATE);
    }
    let mut rows = Rows::new(out, op, query.cursor, u32::MAX)?;
    let mut index = 0_usize;
    host.builtin(&mut |row: &BuiltinRow<'_>| {
        if row.id.is_empty() {
            return ControlFlow::Continue(());
        }
        let number = index;
        index += 1;
        if !checked.leaves_out(number) {
            return ControlFlow::Continue(());
        }
        rows.push(|| {
            Record::ServiceDevBuiltin(wire::ServiceDevBuiltin {
                id: clip(row.id, wire::SERVICE_DEV_BUILTIN_ID_MAX_BYTES),
                version: clip(row.version, wire::SERVICE_DEV_BUILTIN_VERSION_MAX_BYTES),
            })
        })
    })?;
    rows.finish(true, generation)
}
