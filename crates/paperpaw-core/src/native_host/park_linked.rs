//! Linked native plugins parked at their admission (#248, #2638,
//! `docs/measurements/parked-plugins.md`, phase 1).
//!
//! A linked plugin whose `make` only registered, that holds nothing of the
//! host, whose declarations all lie in the program's image and that has no
//! role another plugin's callback could call is parked right after its own
//! `make`: its declarations are published as for any plugin, then its
//! instance is dropped and its lease keeps a [`Recipe`] that makes it again
//! on first use (`lease_slot`). It is never parked again once made.
use super::{
    FixedTables, FlatHost, HostResources, flat::FlatDepth, lease_slot::Unpark,
    lease_slot::Unparked, linked_entry, native_dispatch::NativeDispatch,
    native_instances::NativeIdentity, native_instances::NativeInstance,
    raw_declarations::RawDeclarations, registrations::Record, registrations::Registration,
    resources::OwnerSeat, retained::Retained, shared::Shared, shell,
};
use alloc::boxed::Box;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, PpEntry, Services};
use paperpaw_native::metadata;

/// What a shell gives for a linked plugin it lets the kernel park: what
/// [`crate::Kernel::register_linked`] takes, kept to host the plugin again.
pub struct ParkArgs<S> {
    pub(crate) entry: &'static PpEntry,
    pub(crate) metadata: &'static [u8],
    pub(crate) host: metadata::Host<'static>,
    pub(crate) id: &'static str,
    /// Makes the shell's services handle anew for each hosting.
    pub(crate) services: fn() -> S,
    pub(crate) resources: HostResources,
    pub(crate) teardown: shell::Report<S>,
    pub(crate) image: (*const u8, usize),
    pub(crate) fixed: FixedTables,
}

/// Makes a parked linked plugin again: hosted under its old owner key and
/// heap row, `make` run once more, its batch checked against the one of its
/// admission, its dispatch built as at the admission.
pub(crate) struct Recipe<S: Services + 'static> {
    args: ParkArgs<S>,
    seat: OwnerSeat,
    report: fn(&S, u32, i32),
    depth: FlatDepth,
    flat_host: Shared<FlatHost>,
    identity: NativeIdentity,
    events: super::event_tags::EventTags,
    /// The admission's batch: its image ranges and registrations.
    hash: u64,
}

impl<S: Services + 'static> Recipe<S> {
    #[allow(clippy::too_many_arguments, reason = "what the dispatch is made of")]
    pub(crate) fn new(
        args: ParkArgs<S>,
        seat: OwnerSeat,
        report: fn(&S, u32, i32),
        depth: FlatDepth,
        flat_host: Shared<FlatHost>,
        identity: NativeIdentity,
        events: super::event_tags::EventTags,
        hash: u64,
    ) -> Self {
        Self {
            args,
            seat,
            report,
            depth,
            flat_host,
            identity,
            events,
            hash,
        }
    }

    fn made(&self) -> Result<Box<dyn NativeInstance>, Unparked> {
        let args = &self.args;
        let admitted = linked_entry::check(args.entry, args.metadata, &args.host)
            .map_err(|_| Unparked::Refused)?;
        let resources = args.resources.try_clone().ok_or(Unparked::NoMemory)?;
        // SAFETY: the contract `register_linked_parked` took these under:
        // the factory, its static code and the image stay for the whole run.
        let hosted = unsafe {
            admitted.hosted_for_owner(
                (args.services)(),
                args.id,
                args.teardown,
                resources,
                args.image,
                self.seat,
            )
        }
        .map_err(unparked)?;
        let (retained, tokens, raw) =
            Retained::make_native_stored(hosted, drawn_tokens, |_, _| {}).map_err(unparked)?;
        // The same batch as at the admission, and nothing read to make it:
        // else the plugin is withdrawn (the declarations published are its
        // admission's).
        if !retained.host().make_registered_only()
            || batch_hash(&raw, &retained, &self.depth) != Some(self.hash)
        {
            return Err(Unparked::Refused);
        }
        let events = super::native_install::check_registrations(&retained, &self.depth, &tokens)
            .map_err(unparked)?;
        if events != self.events {
            return Err(Unparked::Refused);
        }
        let dispatch = NativeDispatch::with_events(
            retained,
            self.depth.try_clone().ok_or(Unparked::NoMemory)?,
            self.flat_host.try_clone().ok_or(Unparked::NoMemory)?,
            self.report,
            self.identity.try_clone().ok_or(Unparked::NoMemory)?,
            events,
        );
        let dispatch = super::shared::try_box(dispatch).ok_or(Unparked::NoMemory)?;
        Ok(dispatch)
    }
}

impl<S: Services + 'static> Unpark for Recipe<S> {
    fn unpark(&self) -> Result<Box<dyn NativeInstance>, Unparked> {
        self.made()
    }
    fn subscribed(&self, tag: u32) -> bool {
        self.events.contains(tag)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        let image = self.args.fixed.image;
        visit((!image.is_empty()).then_some(image));
    }
}

impl crate::Kernel {
    /// The recipe to park the plugin just made under `key` with, where it
    /// may be parked (the four conditions of the design, checked here):
    /// `None` where it may not, or where what parking needs has no room
    /// (it then runs as any plugin).
    #[allow(clippy::too_many_arguments, reason = "the admission's parts")]
    pub(super) fn park_recipe<S: Services + 'static, M: 'static>(
        &mut self,
        key: u64,
        retained: &Retained<S, M>,
        raw: &RawDeclarations,
        depth: &FlatDepth,
        events: super::event_tags::EventTags,
        report: fn(&S, u32, i32),
        args: ParkArgs<S>,
    ) -> Option<Recipe<S>> {
        let host = retained.host();
        // 1 and 2: `make` only registered, the owner holds nothing.
        if !host.make_registered_only() || host.owner_idle() != Ok(true) {
            return None;
        }
        // A plugin told at the start would be made at once.
        if events.contains(super::registrations::EVENT_STARTED_TAG) {
            return None;
        }
        // 3: every declaration in the image (none freed with the instance).
        let hash = batch_hash(raw, retained, depth)?;
        // 4: no role a nested call reaches.
        let mut nested = false;
        raw.visit(args.id, host.image(), &mut |record| {
            nested |= nested_role(&record)
        })
        .ok()?;
        if nested {
            return None;
        }
        let identity = self.shared.native_instances.identity(key).ok()?;
        Some(Recipe::new(
            args,
            host.seat(),
            report,
            depth.try_clone()?,
            self.shared_flat_host().ok()?,
            identity,
            events,
            hash,
        ))
    }

    /// The plugin under `key` published with a parked lease: its
    /// declarations as for any plugin, then its instance dropped (its
    /// `make`'s state destroyed, its owner key retired until it is made).
    pub(super) fn bind_parked<S: Services + 'static, M: 'static>(
        &mut self,
        key: u64,
        retained: Retained<S, M>,
        recipe: Recipe<S>,
        (owned, raw, folder): (
            alloc::vec::Vec<super::native_install::Owned>,
            RawDeclarations,
            &str,
        ),
        owner: &paperpaw_sdk::Id,
    ) -> Result<(), shell::Error> {
        let clock = self
            .shared
            .native_instances
            .unpark_clock()
            .map_err(shell::Error::Status)?;
        let recipe: Box<dyn Unpark> =
            super::shared::try_box(recipe).ok_or(shell::Error::Allocation)?;
        let lease = super::native_instances::NativeLease::parked(recipe, clock)
            .map_err(shell::Error::Status)?;
        self.bind_lease(key, &lease, (owned, raw, folder), owner)?;
        drop(retained);
        Ok(())
    }
}

/// Why a hosting failed, as an unpark, read as a call's status
/// (`native_dispatch::status`): no room or busy (the old owner not gone yet,
/// a depth or lifecycle in use, the plugin's own `make` out of memory) waits
/// and tries again; anything else withdraws the plugin.
fn unparked(error: shell::Error) -> Unparked {
    match super::native_dispatch::status(error) {
        ERR_NO_MEMORY | ERR_BUSY => Unparked::NoMemory,
        _ => Unparked::Refused,
    }
}

/// The tokens of what the plugin draws itself, which its draw registrations
/// may name (as `map_declarations` collects them, with nothing copied).
fn drawn_tokens(view: &super::declaration_checks::View<'_>) -> Result<alloc::vec::Vec<u32>, i32> {
    use super::registrations as wire;
    let mut tokens = alloc::vec::Vec::new();
    let mut result = Ok(());
    view.visit(&mut |record| {
        if let Record::ScreenSpec(wire::ScreenSpec { token, .. })
        | Record::WidgetSpec(wire::WidgetSpec { token, .. })
        | Record::StatusItem(wire::StatusItem { token, .. })
        | Record::StatusLine(wire::StatusLine { token, .. }) = record
        {
            if tokens.try_reserve(1).is_err() {
                result = Err(ERR_NO_MEMORY);
            } else {
                tokens.push(token);
            }
        }
    })?;
    result.map(|()| tokens)
}

/// The batch `make` submitted, as a number: the image range of each
/// declaration batch and every registration. `None` where a declaration
/// batch is not in the image (copied: it would be freed with the instance).
pub(crate) fn batch_hash<S: Services + 'static, M: 'static>(
    raw: &RawDeclarations,
    retained: &Retained<S, M>,
    depth: &FlatDepth,
) -> Option<u64> {
    let mut hash = Fnv::default();
    for batch in &raw.batches {
        let range = batch.image?;
        hash.bytes(&range.offset.to_le_bytes());
        hash.bytes(&range.len.to_le_bytes());
        hash.bytes(&batch.entries.to_le_bytes());
    }
    retained
        .registrations(depth.as_depth(), &mut |index, record| {
            hash.bytes(&index.to_le_bytes());
            // Each field as it is, no formatting (none collide, none pulled in).
            let (kind, tag, token) = match record {
                Registration::Event { event_tag, token } => (0_u8, event_tag, token),
                Registration::Draw { token } => (1, 0, token),
            };
            hash.bytes(&[kind]);
            hash.bytes(&tag.to_le_bytes());
            hash.bytes(&token.to_le_bytes());
        })
        .ok()?;
    Some(hash.0)
}

/// FNV-1a, 64 bits: no allocation, the same on every target.
struct Fnv(u64);
impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}
impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// Whether a declaration gives the plugin a role another plugin's callback,
/// the host's drawing or a format call reaches it by (condition 4): such a
/// plugin is never parked (a nested call cannot make it).
pub(crate) fn nested_role(record: &Record<'_>) -> bool {
    matches!(
        record,
        Record::RegistrationStage(_)
            | Record::FormatSpec(_)
            | Record::LookupSpec(_)
            | Record::WidgetSpec(_)
            | Record::WidgetPlace(_)
            | Record::StatusItem(_)
            | Record::StatusLine(_)
            | Record::StatusPlace(_)
            | Record::ReaderSpec(_)
            | Record::BookBadgeSpec(_)
            | Record::BookFieldSpec(_)
    )
}

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
#[path = "park_linked_tests.rs"]
mod tests;
