//! Host side of the service `core.registry` (`docs/abi/registry.md` section 2):
//! the READ side of REG, ops 1-22, built family by family.
//!
//! The backend is the generic [`Backend`](super::resources) function and holds
//! no state: a call decodes its input, asks the frame's [`RegistryView`] for
//! the rows of the op and frames them. The view is where the host's tables
//! are read (the environment's registries, the plugin list, the declaration
//! store); the backend owns the contract, which is the same for every op:
//!
//! - a list op takes one `service.reg_query`, answers up to 32 whole rows and
//!   exactly one `service.end` ([`RegistryPage`]); a stale or unknown
//!   generation is `ERR_STATE` with `out` untouched;
//! - a detail op takes one `service.reg_key`, answers the records of its row
//!   in order and no `end`; an unknown key is `ERR_NOT_FOUND`;
//! - a scope that names nothing known is an empty page, not an error.
//!
//! Memory (X4): a call uses the reply buffer the dispatcher lends and a few
//! hundred bytes of stack; no state is kept between pages. A page counts the
//! rows once to give `total` and then walks them again from the cursor, so a
//! row is visited twice per page, never stored. A row that costs much to
//! build (a setting's) is built only where the page takes it ([`Row::frames`]).
use super::{
    codec,
    host_services::Caller,
    registrations::{self as wire, Record},
    registry_page::RegistryPage,
};
use crate::abi7::{
    context::Frame,
    services_wire::{ServiceOp, core_registry_v1 as schema},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED};

mod bindings;
pub(crate) mod fixed;
mod items;
mod menu_native;
mod menus;
#[cfg(test)]
mod menus_tests;
mod native_extra;
mod ordered;
mod plugins;
mod presets;
pub(in crate::native_host) mod rules;
mod settings;
#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
mod settings_described_tests;
#[cfg(test)]
mod settings_native_tests;
#[cfg(test)]
mod settings_tests;
mod setup;
mod sources;
mod status;
#[cfg(test)]
mod tests;
mod widgets;

pub(crate) use sources::Sources;

pub(crate) use crate::abi7::context::registry_views::{Asker, RegistryView, Row};

impl Row for Record<'_> {
    fn framed_len(&self) -> Result<usize, i32> {
        codec::framed_len(self)
    }
    fn encode(&self, out: &mut [u8]) -> Result<usize, i32> {
        codec::encode_one(self, out)
    }
}

/// The `core.registry` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let view = frame.registry()?;
    let asker = Asker {
        owner: who.owner,
        plugin: who.plugin,
    };
    match op.id {
        schema::OP_SETTING | schema::OP_PLUGIN | schema::OP_PRESET => {
            let key: wire::ServiceRegKey<'_> = codec::single(input)?;
            detail(view, op, key.key, &asker, out)
        }
        schema::OP_SETTINGS..=schema::OP_STATUS => {
            let query: wire::ServiceRegQuery<'_> = codec::single(input)?;
            list(view, op, &query, &asker, out)
        }
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// A list op: the page from the cursor, then the one `end`.
fn list(
    view: &dyn RegistryView,
    op: &ServiceOp,
    query: &wire::ServiceRegQuery<'_>,
    asker: &Asker<'_>,
    out: &mut [u8],
) -> Result<usize, i32> {
    // A flag bit this build does not know is not ignored (`records.md`).
    if query.flags & !query_flags_of(op.id) != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    let generation = view.generation();
    let scope = scope_of(op.id, query);
    let mut total = 0_u32;
    // A row that cannot be framed (an id over its field's cap, which no
    // declaration check bounds for plugin ids) is skipped, here and in the
    // page walk alike, so `total`, the cursor and the rows agree.
    view.rows(op.id, scope, query.flags, asker, &mut |row| {
        if row.frames() {
            total = total.saturating_add(1);
        }
        ControlFlow::Continue(())
    })?;
    let room = op.max_reply.min(out.len());
    let out = out.get_mut(..room).ok_or(ERR_INVALID)?;
    let mut page = RegistryPage::new(query, generation, total, out)?;
    let mut index = 0_u32;
    let mut failure = None;
    view.rows(op.id, scope, query.flags, asker, &mut |row| {
        if !row.frames() {
            return ControlFlow::Continue(());
        }
        let at = index;
        index = index.saturating_add(1);
        if at < query.cursor {
            return ControlFlow::Continue(());
        }
        match page.push(row) {
            Ok(flow) => flow,
            Err(error) => {
                failure = Some(error);
                ControlFlow::Break(())
            }
        }
    })?;
    if let Some(error) = failure {
        // A row past `total` means the tables changed between the two walks.
        return Err(if error == ERR_INVALID {
            ERR_STATE
        } else {
            error
        });
    }
    page.finish()
}

/// The `reg_query.flags` bits an op knows: op 12's `all` and `declared`, op
/// 18's placement bit, op 19's previews bit; no other op reads any.
const fn query_flags_of(op: u32) -> u32 {
    match op {
        schema::OP_MENU => 0b11,
        schema::OP_VALUES | schema::OP_WIDGETS => 0b1,
        _ => 0,
    }
}

/// The scope the view reads. Op 18 names a preset or, with `flags` bit 0, a
/// placement (an id with an `@`, which no preset id has): a scope of the
/// other kind than its flag says names nothing.
fn scope_of<'a>(op: u32, query: &wire::ServiceRegQuery<'a>) -> &'a str {
    let placement = query.flags & 1 != 0;
    if op == schema::OP_VALUES && placement != query.scope.contains('@') {
        ""
    } else {
        query.scope
    }
}

/// A detail op: the records of the row, each framed after the last.
fn detail(
    view: &dyn RegistryView,
    op: &ServiceOp,
    key: &str,
    asker: &Asker<'_>,
    out: &mut [u8],
) -> Result<usize, i32> {
    let room = op.max_reply.min(out.len());
    let out = out.get_mut(..room).ok_or(ERR_INVALID)?;
    let mut used = 0;
    view.detail(op.id, key, asker, &mut |row| {
        // A row that cannot be framed (a plugin id past its field) is not
        // listed by the walks either: as if it were not there.
        if row.framed_len().is_err() {
            return Err(ERR_NOT_FOUND);
        }
        let rest = out.get_mut(used..).ok_or(ERR_INVALID)?;
        used += row.encode(rest)?;
        Ok(())
    })?;
    if used == 0 {
        Err(ERR_NOT_FOUND)
    } else {
        Ok(used)
    }
}
