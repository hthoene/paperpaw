//! Reads to a file (`sockets.md` 2.3): the card rules, the paths in use, the
//! staged file and the rename, and what ends it.
use super::{
    Caller, ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_STATE, Inner, SockHost, State, Want, kind, owned,
    reason, table, wire,
};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE};

/// The most bytes one file read takes (2^40, 2.3).
const LEN_MAX: u64 = 1 << 40;

/// A read to a file under way.
pub(super) struct File {
    root: u32,
    path: alloc::string::String,
    remaining: u64,
    first: bool,
}

/// `read` mode 1. Everything that can fail comes before the first change.
pub(super) fn read(
    host: &dyn SockHost,
    who: &Caller<'_>,
    r: &wire::ServiceSockRead<'_>,
) -> Result<(), i32> {
    if r.flags != 0
        || r.max != 0
        || r.len == 0
        || r.len > LEN_MAX
        || r.path.is_empty()
        || !matches!(r.root, 0 | 1)
    {
        return Err(ERR_INVALID);
    }
    let cap = host.profile().read;
    let mut inner = table(host)?;
    let index = inner.mine(who.owner, r.socket)?;
    let s = &inner.socks[index];
    if s.state != State::Open {
        return Err(ERR_STATE);
    }
    if s.want.is_some() || s.received.is_some() || s.file.is_some() {
        return Err(ERR_BUSY);
    }
    host.root_access(who, r.root, r.path)?;
    if !host.room_for(r.len) {
        return Err(ERR_FULL);
    }
    let path = owned(r.path)?;
    host.claim_save(who.owner, r.root, r.path)?;
    let s = &mut inner.socks[index];
    s.file = Some(File {
        root: r.root,
        path,
        remaining: r.len,
        first: true,
    });
    s.want = Some(Want {
        max: u32::try_from(r.len).unwrap_or(u32::MAX).min(cap),
        exact: false,
    });
    inner.settle_eof(host, index);
    inner.grant(host, index);
    inner.sync_demand(host);
    Ok(())
}

impl Inner {
    /// The read to a file of socket `index` stops: its staged file goes,
    /// the old file stays, the claim is given back. Nothing if there is none.
    pub(super) fn drop_file(&mut self, host: &dyn SockHost, index: usize) {
        let s = &mut self.socks[index];
        if let Some(file) = s.file.take() {
            s.want = None;
            s.granted = false;
            host.release_save(s.owner, file.root, &file.path);
        }
    }

    fn fail_file(&mut self, host: &dyn SockHost, index: usize, why: u32) {
        self.end(host, index, kind::FAILED, why, true);
    }

    /// Bytes of a file read arrived: written to the staged file at once (the
    /// read part is the buffer), the rename and `saved` with the last.
    pub(super) fn file_received(
        &mut self,
        host: &dyn SockHost,
        index: usize,
        bytes: &[u8],
    ) -> Result<usize, i32> {
        let now = host.now_ms();
        let s = &mut self.socks[index];
        let (Some(want), Some(file)) = (s.want, s.file.as_mut()) else {
            return Err(ERR_STATE);
        };
        let taken = bytes
            .len()
            .min(want.max as usize)
            .min(usize::try_from(file.remaining).unwrap_or(usize::MAX));
        if taken == 0 {
            return Ok(0);
        }
        if !host.room_for(taken as u64) {
            self.fail_file(host, index, reason::FULL);
            return Ok(taken);
        }
        // The read part's room was allocated at `connect`: this never grows it.
        s.inbox.extend_from_slice(bytes.get(..taken).unwrap_or(&[]));
        let written = host.staged_write(s.owner, file.root, &file.path, &s.inbox, file.first);
        s.inbox.clear();
        match written {
            Ok(()) => {}
            Err(ERR_NO_SPACE | ERR_FULL) => {
                self.fail_file(host, index, reason::FULL);
                return Ok(taken);
            }
            Err(error) => {
                let why = if error == ERR_NO_MEMORY {
                    reason::NO_MEMORY
                } else {
                    reason::IO
                };
                self.fail_file(host, index, why);
                return Ok(taken);
            }
        }
        file.first = false;
        file.remaining -= taken as u64;
        s.last_move = now;
        let left = want.max - taken as u32;
        if file.remaining == 0 {
            if host.staged_commit(s.owner, file.root, &file.path).is_err() {
                self.fail_file(host, index, reason::IO);
                return Ok(taken);
            }
            // Whole: the claim goes (the staged file is renamed away).
            self.drop_file(host, index);
            let slot = self.slot(host);
            self.socks[index].saved = Some(slot);
        } else if left == 0 {
            // The next part of the read.
            let cap = host.profile().read;
            s.want = Some(Want {
                max: u32::try_from(file.remaining).unwrap_or(u32::MAX).min(cap),
                exact: false,
            });
            s.granted = false;
            self.grant(host, index);
        } else {
            s.want = Some(Want {
                max: left,
                exact: false,
            });
        }
        Ok(taken)
    }
}
