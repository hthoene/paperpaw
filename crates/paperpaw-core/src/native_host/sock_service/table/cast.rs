//! UDP broadcast (`sockets.md` 3): one datagram to the network's broadcast
//! address on up to eight ports, the answers for `wait_ms`, then one `done`.
//!
//! A broadcast is one of the plugin's sockets while it runs: it counts against
//! the same limits, charges its eight answers (8 KiB) to the network pool at
//! the call, and ends with the screen, the run, the network and the plugin.
//! Its answers are read with `data` on its id during the answer's event.
use super::{
    Caller, ERR_INVALID, ERR_NOT_FOUND, ERR_STATE, Inner, PIECE, Slot, SockHost, buffer, codec,
    gate, reason, table, wire,
};
use super::{Record, Vec};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE};

/// `event.broadcast.kind` (4).
pub(crate) mod cast_kind {
    pub(crate) const ANSWER: u32 = 0;
    pub(crate) const DONE: u32 = 1;
    pub(crate) const FAILED: u32 = 2;
}

/// Answers kept per broadcast and the longest one (3).
pub(crate) const ANSWERS: usize = 8;
pub(crate) const ANSWER_MAX: usize = 1_024;
/// The pool charge of a broadcast: its eight answers.
pub(crate) const CHARGE: u32 = (ANSWERS * ANSWER_MAX) as u32;
/// The datagram and the ports (3).
const DATA_MAX: usize = 512;
const PORTS_MAX: usize = 8;
/// `wait_ms` (3).
const WAIT_MIN: u32 = 100;
const WAIT_MAX: u32 = 10_000;
/// The longest `from` (`event.broadcast.from`).
const FROM_MAX: usize = wire::EVENT_BROADCAST_FROM_MAX_BYTES;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Ended,
}

struct Answer {
    /// `None` once the plugin was told.
    slot: Option<Slot>,
    from: [u8; FROM_MAX],
    from_len: u8,
    at: usize,
    len: usize,
}

pub(super) struct Cast {
    id: u32,
    owner: u64,
    token: u32,
    screen: Option<u64>,
    run: Option<u64>,
    run_open: bool,
    state: State,
    deadline: u64,
    /// Pool bytes still held.
    charge: u32,
    /// The answers' bytes, back to back (room for all eight, reserved at the
    /// call).
    arena: Vec<u8>,
    answers: Vec<Answer>,
    /// The answer whose event callback runs.
    held: Option<usize>,
    end: Option<(Slot, u32, u32)>,
}

impl Cast {
    pub(super) fn live(&self) -> bool {
        self.state != State::Ended
    }

    pub(super) fn run(&self) -> Option<u64> {
        self.run
    }

    fn untold(&self) -> bool {
        self.answers.iter().any(|a| a.slot.is_some()) || self.held.is_some()
    }

    fn pending(&self) -> bool {
        self.end.is_some() || self.untold()
    }

    pub(super) fn screen_is(&self, owner: u64, instance: u64) -> bool {
        self.owner == owner && self.screen == Some(instance)
    }

    pub(super) fn run_is(&self, owner: u64, run: u64) -> bool {
        self.owner == owner && self.run == Some(run)
    }

    pub(super) fn owner(&self) -> u64 {
        self.owner
    }
}

/// `broadcast` (3): `sock_broadcast`, then 1-8 `sock_port`. Everything that
/// can fail comes before the first change.
pub(in crate::native_host::sock_service) fn broadcast(
    host: &dyn SockHost,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<u32, i32> {
    let mut head = None;
    let mut ports = [0_u16; PORTS_MAX];
    let mut count = 0_usize;
    wire::host_records_visit(input, 1 + PORTS_MAX, &mut |record| {
        match record {
            Record::ServiceSockBroadcast(b) if head.is_none() => head = Some(b),
            Record::ServiceSockPort(p) if head.is_some() && count < PORTS_MAX => {
                *ports.get_mut(count).ok_or(ERR_INVALID)? =
                    u16::try_from(p.port).map_err(|_| ERR_INVALID)?;
                count += 1;
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    let b = head.ok_or(ERR_INVALID)?;
    let ports = ports.get(..count).ok_or(ERR_INVALID)?;
    if b.data.is_empty()
        || b.data.len() > DATA_MAX
        || !(WAIT_MIN..=WAIT_MAX).contains(&b.wait_ms)
        || ports.is_empty()
        || ports.contains(&0)
    {
        return Err(ERR_INVALID);
    }
    if !host.subscribed(who.owner, wire::EVENT_BROADCAST_TAG) {
        return Err(ERR_STATE);
    }
    let mut arena = buffer(CHARGE)?;
    gate()?;
    let mut answers = Vec::new();
    answers
        .try_reserve_exact(ANSWERS)
        .map_err(|_| ERR_NO_MEMORY)?;
    let mut inner = table(host)?;
    inner.admit(host, who.owner)?;
    gate()?;
    inner.casts.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    let id = inner
        .next_id
        .checked_add(1)
        .ok_or(paperpaw_native::host_glue::ERR_FULL)?;
    if !inner.reserve(host, CHARGE) {
        return Err(ERR_NO_MEMORY);
    }
    // The work run (if any) is kept going while the broadcast lives; the
    // charge goes back when that is refused.
    let run = host.current_run(who.owner);
    if let Some(run) = run
        && let Err(error) = host.resource_opened(run)
    {
        inner.release(host, CHARGE);
        return Err(error);
    }
    let pooled = host.table().pool.capacity() != 0;
    inner.next_id = id;
    arena.clear();
    inner.casts.push(Cast {
        id,
        owner: who.owner,
        token: b.token,
        screen: host.showing_screen(who.owner),
        run,
        run_open: run.is_some(),
        state: State::Running,
        deadline: host.now_ms().saturating_add(u64::from(b.wait_ms)),
        charge: if pooled { CHARGE } else { 0 },
        arena,
        answers,
        held: None,
        end: None,
    });
    let index = inner.casts.len() - 1;
    inner.sync_demand(host);
    if let Some(end) = host.power_end(who.owner, inner.casts[index].run.is_some()) {
        inner.cast_end(host, index, cast_kind::FAILED, end.reason, false);
    } else if !host.network_up() {
        // Without a network the broadcast is over at once (3).
        inner.cast_end(host, index, cast_kind::DONE, reason::NO_NETWORK, false);
    } else if let Err(error) = host.transport().broadcast(id, b.data, ports) {
        let why = if error == ERR_NO_MEMORY {
            reason::NO_MEMORY
        } else {
            reason::IO
        };
        inner.cast_end(host, index, cast_kind::FAILED, why, false);
    }
    inner.sync_demand(host);
    Ok(id)
}

impl Inner {
    pub(super) fn cast_mine(&self, owner: u64, id: u32) -> Option<usize> {
        self.casts
            .iter()
            .position(|c| c.id == id && c.owner == owner && c.live())
    }

    /// End broadcast `index` once with `kind` and `reason`. `abort`: the
    /// transport still has it.
    pub(super) fn cast_end(
        &mut self,
        host: &dyn SockHost,
        index: usize,
        kind: u32,
        reason: u32,
        abort: bool,
    ) {
        if !self.casts[index].live() {
            return;
        }
        if abort {
            host.transport().close(self.casts[index].id);
        }
        let slot = self.slot(host);
        let c = &mut self.casts[index];
        c.state = State::Ended;
        c.end = Some((slot, kind, reason));
        // Answers the plugin is yet to be told of keep their bytes and charge.
        if !c.untold() {
            self.cast_free(host, index);
        }
    }

    fn cast_free(&mut self, host: &dyn SockHost, index: usize) {
        let c = &mut self.casts[index];
        c.arena = Vec::new();
        c.answers = Vec::new();
        let back = core::mem::take(&mut c.charge);
        self.release(host, back);
    }

    pub(super) fn cast_expire(&mut self, host: &dyn SockHost) {
        let now = host.now_ms();
        for index in 0..self.casts.len() {
            if !self.casts[index].live() {
                continue;
            }
            let (owner, background) = (self.casts[index].owner, self.casts[index].run.is_some());
            if let Some(end) = host.power_end(owner, background)
                && end.reason == reason::NETWORK_OFF
            {
                // The user turned the plugin's network off: no answer is waited for.
                self.cast_end(host, index, cast_kind::FAILED, end.reason, true);
            } else if now >= self.casts[index].deadline {
                self.cast_end(host, index, cast_kind::DONE, reason::NONE, true);
            }
        }
    }

    /// When the first live broadcast runs out of its `wait_ms`.
    pub(super) fn cast_next_deadline(&self) -> Option<u64> {
        self.casts
            .iter()
            .filter(|c| c.live())
            .map(|c| c.deadline)
            .min()
    }

    pub(super) fn cast_end_all(
        &mut self,
        host: &dyn SockHost,
        kind: u32,
        why: u32,
        mut chosen: impl FnMut(&Cast) -> bool,
    ) {
        for index in 0..self.casts.len() {
            if self.casts[index].live() && chosen(&self.casts[index]) {
                self.cast_end(host, index, kind, why, true);
            }
        }
    }

    /// The callback of `owner` returned: the answer it read is let go, and the
    /// broadcasts whose last event it was go.
    pub(super) fn cast_delivered(&mut self, host: &dyn SockHost, owner: u64) {
        for index in 0..self.casts.len() {
            if self.casts[index].owner == owner {
                self.casts[index].held = None;
            }
        }
        let mut index = 0;
        while index < self.casts.len() {
            let c = &self.casts[index];
            if c.owner == owner && !c.live() && !c.pending() {
                let back = c.charge;
                self.release(host, back);
                // The plugin has been told of the end: the run lets go of it.
                let c = &mut self.casts[index];
                if let Some(run) = c.run
                    && core::mem::take(&mut c.run_open)
                {
                    host.resource_closed(run);
                }
                self.casts.remove(index);
            } else {
                index += 1;
            }
        }
    }

    /// The plugin stopped: its broadcasts go without events.
    /// Whether `owner` has a cast (#248, D3).
    pub(super) fn cast_owner_holds(&self, owner: u64) -> bool {
        self.casts.iter().any(|c| c.owner == owner)
    }

    pub(super) fn cast_owner_stopped(&mut self, host: &dyn SockHost, owner: u64) -> u32 {
        let mut freed = 0_u32;
        let mut index = 0;
        while index < self.casts.len() {
            if self.casts[index].owner != owner {
                index += 1;
                continue;
            }
            let c = &mut self.casts[index];
            if let Some(run) = c.run
                && core::mem::take(&mut c.run_open)
            {
                host.resource_closed(run);
            }
            if c.live() {
                host.transport().close(c.id);
            }
            freed = freed.saturating_add(c.charge);
            self.casts.remove(index);
        }
        freed
    }
}

/// The transport received a datagram for broadcast `id`, from `from` (the
/// sender's address and port as text). The transport's data is not trusted:
/// an answer over 1,024 bytes, or a `from` over 64 bytes or not UTF-8, is
/// dropped whole (a cut-off answer would be malformed protocol data) and does
/// not count toward the eight ([`SockTable::take_dropped`]). The eighth
/// answer ends the broadcast with `done` once it is told; later and late
/// datagrams are ignored.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn answered(host: &dyn SockHost, id: u32, from: &[u8], bytes: &[u8]) -> Result<(), i32> {
    let mut inner = table(host)?;
    let Some(index) = inner.casts.iter().position(|c| c.id == id) else {
        return Ok(());
    };
    let c = &inner.casts[index];
    if !c.live() || c.answers.len() >= ANSWERS {
        return Ok(());
    }
    let (Some(text), true) = (copy_from(from), bytes.len() <= ANSWER_MAX) else {
        host.table()
            .dropped
            .set(host.table().dropped.get().saturating_add(1));
        return Ok(());
    };
    let slot = inner.slot(host);
    let c = &mut inner.casts[index];
    let at = c.arena.len();
    // The room for eight answers of 1,024 bytes was reserved at the call.
    c.arena.extend_from_slice(bytes);
    c.answers.push(Answer {
        slot: Some(slot),
        from: text.0,
        from_len: text.1,
        at,
        len: bytes.len(),
    });
    if c.answers.len() >= ANSWERS {
        // No further answer could be told: the broadcast is over.
        inner.cast_end(host, index, cast_kind::DONE, reason::NONE, true);
        inner.sync_demand(host);
    }
    Ok(())
}

/// `from` as the fixed text of an answer; `None` when it is over 64 bytes or
/// not UTF-8.
fn copy_from(from: &[u8]) -> Option<([u8; FROM_MAX], u8)> {
    core::str::from_utf8(from).ok()?;
    let mut text = [0_u8; FROM_MAX];
    text.get_mut(..from.len())?.copy_from_slice(from);
    Some((text, u8::try_from(from.len()).ok()?))
}

/// `data` for a broadcast id: the bytes of the answer whose event runs.
pub(super) fn data(
    inner: &Inner,
    owner: u64,
    at: &wire::ServiceSockAt,
    out: &mut [u8],
) -> Result<usize, i32> {
    let c = inner
        .casts
        .iter()
        .find(|c| c.id == at.socket && c.owner == owner)
        .ok_or(ERR_NOT_FOUND)?;
    let answer = c
        .held
        .and_then(|held| c.answers.get(held))
        .ok_or(ERR_STATE)?;
    let all = c
        .arena
        .get(answer.at..answer.at + answer.len)
        .ok_or(ERR_INVALID)?;
    let from = (at.offset as usize).min(all.len());
    let mut take = (all.len() - from).min(PIECE);
    loop {
        let bytes = all.get(from..from + take).ok_or(ERR_INVALID)?;
        match codec::encode_one(
            &Record::ServiceSockBytes(wire::ServiceSockBytes { data: bytes }),
            out,
        ) {
            Err(ERR_NO_SPACE) if take > 0 => take /= 2,
            other => return other,
        }
    }
}

/// Which event of a broadcast is next.
#[derive(Clone, Copy)]
pub(super) enum Pick {
    Answer(usize),
    End,
}

/// The oldest event of `owner`'s broadcasts: its stamp, the broadcast and
/// the event.
pub(super) fn scan(inner: &Inner, owner: u64) -> Option<(u64, usize, Pick)> {
    let mut best: Option<(u64, usize, Pick)> = None;
    for (index, c) in inner
        .casts
        .iter()
        .enumerate()
        .filter(|(_, c)| c.owner == owner)
    {
        let answers = c
            .answers
            .iter()
            .enumerate()
            .filter_map(|(n, a)| a.slot.map(|s| (s.stamp, Pick::Answer(n))));
        let end = c.end.map(|(s, _, _)| (s.stamp, Pick::End));
        for (stamp, pick) in answers.chain(end) {
            if best.is_none_or(|(b, _, _)| stamp < b) {
                best = Some((stamp, index, pick));
            }
        }
    }
    best
}

/// Encode the event `pick` of broadcast `index` into `out` and consume it.
pub(super) fn encode(
    inner: &mut Inner,
    index: usize,
    pick: Pick,
    out: &mut [u8],
) -> Result<usize, i32> {
    let c = &inner.casts[index];
    let used = match pick {
        Pick::Answer(n) => {
            let a = c.answers.get(n).ok_or(ERR_NOT_FOUND)?;
            let from = a
                .from
                .get(..usize::from(a.from_len))
                .and_then(|b| core::str::from_utf8(b).ok())
                .ok_or(ERR_INVALID)?;
            codec::encode_event(
                &Record::EventBroadcast(wire::EventBroadcast {
                    now_ms: a.slot.map_or(0, |s| s.at),
                    socket: c.id,
                    token: c.token,
                    kind: cast_kind::ANSWER,
                    reason: 0,
                    len: a.len as u32,
                    from,
                }),
                out,
            )?
        }
        Pick::End => {
            let (slot, kind, why) = c.end.ok_or(ERR_NOT_FOUND)?;
            codec::encode_event(
                &Record::EventBroadcast(wire::EventBroadcast {
                    now_ms: slot.at,
                    socket: c.id,
                    token: c.token,
                    kind,
                    reason: why,
                    len: 0,
                    from: "",
                }),
                out,
            )?
        }
    };
    let c = &mut inner.casts[index];
    match pick {
        Pick::Answer(n) => {
            if let Some(a) = c.answers.get_mut(n) {
                a.slot = None;
            }
            c.held = Some(n);
        }
        Pick::End => c.end = None,
    }
    Ok(used)
}
