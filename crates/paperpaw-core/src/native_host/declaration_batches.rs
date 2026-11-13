//! An owner's batches: references and an index while it is staged (and for
//! an owner with a batch of its own), or only where each batch starts in a
//! read-only image (#248). The index is one box an image-only owner's row
//! does not keep (#248, B).

use super::{DeclRef, Slot, Starts, batch_bytes, scan};
use alloc::{boxed::Box, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};

/// An indexed owner's batch references and records: apart, so that an
/// image-only owner's row keeps no room for them (#248, B).
#[derive(Default)]
pub(super) struct Indexed {
    pub(super) refs: Vec<DeclRef>,
    /// Every indexed record of the owner in one list, grouped by kind, and
    /// inside a kind in declaration order.
    pub(super) slots: Vec<Slot>,
}

/// Where an owner's batches are.
pub(super) enum Batches {
    /// Each batch's reference and the index of its records: every owner while
    /// it is staged, and an owner with a batch of its own bytes after (`None`
    /// before its first batch).
    Refs(Option<Box<Indexed>>),
    /// Where each batch starts in the owner's read-only image (an image-only
    /// owner: no slots either; its records of a kind are found by walking the
    /// batches' frames, `declaration_scan.rs`), with the kinds they declare:
    /// a batch without the kind looked for is not walked (#248, R1). Packed
    /// where the batches allow it (`declaration_starts.rs`, #248, D5).
    Starts(Starts),
}

impl Batches {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Refs(indexed) => indexed.as_ref().map_or(0, |indexed| indexed.refs.len()),
            Self::Starts(starts) => starts.len(),
        }
    }

    /// The references (none once the batches are only starts).
    #[cfg(test)]
    pub(super) fn refs(&self) -> &[DeclRef] {
        match self {
            Self::Refs(Some(indexed)) => &indexed.refs,
            Self::Refs(None) | Self::Starts(..) => &[],
        }
    }

    /// The indexed records (none once the batches are only starts).
    pub(super) fn slots(&self) -> &[Slot] {
        match self {
            Self::Refs(Some(indexed)) => &indexed.slots,
            Self::Refs(None) | Self::Starts(..) => &[],
        }
    }

    /// The references and records, while the owner is staged (`ERR_STATE`
    /// once its batches are only starts), made at the first batch
    /// (`ERR_NO_MEMORY` without room for them).
    pub(super) fn indexed_mut(&mut self) -> Result<&mut Indexed, i32> {
        match self {
            Self::Refs(Some(indexed)) => Ok(indexed),
            Self::Refs(empty) => Ok(empty
                .insert(super::super::shared::try_box(Indexed::default()).ok_or(ERR_NO_MEMORY)?)),
            Self::Starts(..) => Err(ERR_STATE),
        }
    }

    /// The bytes of batch `batch`: in `image`, or its own.
    pub(super) fn bytes<'a>(&'a self, batch: usize, image: &'a [u8]) -> Result<&'a [u8], i32> {
        match self {
            Self::Refs(indexed) => {
                let refs = indexed.as_ref().map_or(&[][..], |indexed| &indexed.refs);
                batch_bytes(refs.get(batch).ok_or(ERR_INVALID)?, image)
            }
            Self::Starts(starts) => {
                let start = usize::try_from(starts.start(batch).ok_or(ERR_INVALID)?)
                    .map_err(|_| ERR_INVALID)?;
                scan::batch_at(image, start)
            }
        }
    }
}
