//! One native declaration entry, with owned mapping before binding and publication.
use super::{
    flat::FlatDepth,
    native_dispatch::NativeDispatch,
    native_instances::NativeLease,
    native_screen_factory::NativeScreenSpec,
    native_values::{self, Callback, Value},
    registrations::{self as wire, Record, Registration},
    retained::Retained,
    shell,
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED, Services};
use paperpaw_sdk::{Contribution, Id, IdError, RegisterError};

/// Owned mapping results; future declaration families extend this one dispatch.
enum Owned {
    Screen(Callback),
    Action(Callback),
    Setting(paperpaw_sdk::SettingSpec),
    Group(paperpaw_sdk::SettingGroupSpec),
}

fn map_record(owner: &str, record: Record<'_>) -> Result<Option<Owned>, i32> {
    match record {
        Record::ScreenSpec(_)
        | Record::ActionSpec(_)
        | Record::SettingSpec(_)
        | Record::SettingGroup(_) => match native_values::from_record(owner, record)? {
            Value::Screen(callback) => Ok(Some(Owned::Screen(callback))),
            Value::Action(callback) => Ok(Some(Owned::Action(callback))),
            Value::Setting(spec) => Ok(Some(Owned::Setting(spec))),
            Value::Group(group) => Ok(Some(Owned::Group(group))),
        },
        // The extras (section, editor, check) are presentation the settings
        // page reads from the declaration store; the store keeps the record.
        Record::SettingExtra(_) => Ok(None),
        Record::WidgetSpec(_)
        | Record::WidgetPlace(_)
        | Record::StatusItem(_)
        | Record::StatusLine(_)
        | Record::StatusPlace(_) => Ok(None),
        _ => Err(ERR_UNSUPPORTED),
    }
}

impl crate::Kernel {
    /// Admit a native owner and publish all supported declarations together.
    /// The shell supplies its existing device-wide callback depth and diagnostic
    /// function. Other declaration families are refused as a whole; their host
    /// adapters must register them through the same declaration transaction.
    ///
    /// # Errors
    /// Returns admission, allocation or declaration errors without publishing a
    /// partial screen set. The consumed host is destroyed on every failed install.
    pub fn register_native<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        self.shared
            .native_instances
            .adopt_pool(
                host.resource_identity()
                    .ok_or(shell::Error::Status(ERR_NO_MEMORY))?,
            )
            .map_err(shell::Error::Status)?;
        if self.shared.registry.native_declarations.is_none() {
            self.shared.registry.native_declarations = Some(
                super::shared::Shared::try_new(
                    super::native_declarations::NativeDeclarations::default(),
                )
                .map_err(|_| shell::Error::Allocation)?,
            );
        }
        let depth = depth.try_clone().ok_or(shell::Error::Allocation)?;
        let (retained, (owner, owned, tokens), raw) =
            Retained::make_native_stored(host, map_declarations)?;
        // The callback registrations are admitted whole or the owner is not:
        // none is dropped unrouted.
        check_registrations(&retained, &depth, &tokens)?;
        let key = self
            .shared
            .native_instances
            .insert(
                retained
                    .host()
                    .resource_identity()
                    .ok_or(shell::Error::Status(ERR_NO_MEMORY))?,
                owner
                    .try_clone()
                    .ok_or(shell::Error::Status(ERR_NO_MEMORY))?,
                None,
                true,
            )
            .map_err(shell::Error::Status)?;
        let result = self.bind_native(key, retained, depth, (owned, raw), &owner, report);
        if result.is_err() {
            self.shared
                .native_instances
                .abandon(key)
                .map_err(shell::Error::Status)?;
        }
        result
    }

    /// Revoke first; store cleanup, screen closure and factory removal must
    /// precede resource retirement. Busy cleanup retains a pending row.
    pub(crate) fn begin_native_unload(
        &mut self,
        key: u64,
        owner: &Id,
    ) -> Option<alloc::string::String> {
        if self.shared.native_instances.stop(key).is_err() {
            return None;
        }
        if let Some(declarations) = &self.shared.registry.native_declarations {
            let Ok(mut store) = declarations.store.try_borrow_mut() else {
                return None;
            };
            store.remove_host_owner(key);
            if declarations.catalog.prune(&store).is_err() {
                return None;
            }
        }
        if self.native_owner_stopped(key).is_err() {
            return None;
        }
        self.remove_retired_native_screens(owner);
        self.shared
            .registry
            .screens
            .remove_native_owner(owner.as_str());
        // The actions hold the plugin's lease; the settings and groups it
        // defined go with it (the reader's values stay for its next load).
        self.shared.actions.remove_plugin(owner.as_str());
        self.shared.settings.remove_plugin(owner.as_str());
        // The main loop retries a revoked row and requests a wake meanwhile.
        match self.shared.native_instances.retire(key) {
            Ok(replacement) => replacement,
            Err(status) => {
                if self
                    .shared
                    .native_instances
                    .retirement_failed(key, status, self.now.millis())
                {
                    self.warn_runtime("native owner cleanup failed; retry delayed");
                }
                None
            }
        }
    }

    fn queue_native_dialog_close(&self, dialog: u32) {
        if let Err(status) = self
            .shared
            .stack_rows
            .defer_close(dialog, self.shared.stack_rows.generation())
        {
            self.shared.stack_rows.note_close_error(status);
        }
    }

    fn native_owner_stopped(&self, owner: u64) -> Result<(), i32> {
        if let Some(store) = self.shared.native_instances.ui_store() {
            store.owner_dialogs(owner, &mut |dialog| self.queue_native_dialog_close(dialog))?;
            store.owner_stopped(owner)?;
        }
        Ok(())
    }

    pub(crate) fn native_instance_closed(&self, instance: u32) -> Result<(), i32> {
        if let Some(store) = self.shared.native_instances.ui_store() {
            store.dialog_closed(instance)?;
            store.asker_instance_closed(instance, &mut |cancelled| {
                if let Some(dialog) = cancelled.dialog {
                    self.queue_native_dialog_close(dialog);
                }
            })?;
            store.instance_closed(instance)?;
        }
        Ok(())
    }

    pub(crate) fn retry_native_unloads(&mut self) {
        // The same driver handles HostServices' orphan queue and the retained
        // rows whose lifecycle cleanup has not completed. No second queue.
        if let Err(status) = self
            .shared
            .native_instances
            .retry_orphaned_retirements(self.now.millis())
            && status != paperpaw_native::host_glue::ERR_BUSY
        {
            self.warn_runtime(if status == paperpaw_native::host_glue::ERR_FULL {
                "native retirement queue full; cleanup could not be queued"
            } else {
                "orphaned native owner cleanup failed; retry delayed"
            });
        }
        let mut after = None;
        while let Some((key, owner)) = self.shared.native_instances.pending_unload(after) {
            after = Some(key);
            if !self
                .shared
                .native_instances
                .retirement_ready(key, self.now.millis())
            {
                continue;
            }
            let Some(owner) = owner.try_clone() else {
                continue;
            };
            self.unload_plugin(&owner);
        }
    }

    pub(crate) fn native_unload_due(&self) -> Option<paperpaw_platform::Instant> {
        if self.shared.stack_rows.closes_pending() {
            return Some(self.now);
        }
        self.shared
            .native_instances
            .retirement_due(self.now.millis())
            .map(paperpaw_platform::Instant)
    }

    fn bind_native<S: Services + 'static, M: 'static>(
        &mut self,
        key: u64,
        retained: Retained<S, M>,
        depth: FlatDepth,
        mapped: (Vec<Owned>, super::raw_declarations::RawDeclarations),
        owner: &Id,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        let (owned, raw) = mapped;
        let mut flat_host = self.flat_host();
        flat_host.directory = self
            .shared
            .registry
            .native_declarations
            .as_ref()
            .map(|directory| directory.downgrade().ok_or(shell::Error::Allocation))
            .transpose()?;
        let instance = NativeLease::try_new(NativeDispatch::new(
            retained,
            depth,
            flat_host,
            report,
            self.shared
                .native_instances
                .identity(key)
                .map_err(shell::Error::Status)?,
        ))
        .map_err(shell::Error::Status)?;
        let mut staging = crate::registry::Staging::default();
        for value in owned {
            match value {
                Owned::Screen(callback) => {
                    let screen = instance
                        .try_clone()
                        .and_then(|lease| NativeScreenSpec::new(callback, lease))
                        .map_err(shell::Error::Status)?;
                    staging
                        .native_screen(owner, screen)
                        .map_err(shell::Error::Status)?;
                }
                Owned::Action(callback) => {
                    let action = instance
                        .try_clone()
                        .and_then(|lease| super::native_action::spec(callback, lease))
                        .map_err(shell::Error::Status)?;
                    let action = super::shared::try_box(action)
                        .ok_or(shell::Error::Status(ERR_NO_MEMORY))?;
                    staging
                        .native(owner, Contribution::Action(action))
                        .map_err(shell::Error::Status)?;
                }
                Owned::Setting(spec) => {
                    let spec =
                        super::shared::try_box(spec).ok_or(shell::Error::Status(ERR_NO_MEMORY))?;
                    staging
                        .native(owner, Contribution::Setting(spec))
                        .map_err(shell::Error::Status)?;
                }
                Owned::Group(group) => {
                    let group =
                        super::shared::try_box(group).ok_or(shell::Error::Status(ERR_NO_MEMORY))?;
                    staging
                        .native(
                            owner,
                            Contribution::SettingGroup(paperpaw_sdk::SettingGroup::from_boxed(
                                group,
                            )),
                        )
                        .map_err(shell::Error::Status)?;
                }
            }
        }
        self.shared
            .native_instances
            .install(key, instance.try_clone().map_err(shell::Error::Status)?)
            .map_err(shell::Error::Status)?;
        let declarations = self
            .shared
            .registry
            .native_declarations
            .as_ref()
            .and_then(super::shared::Shared::try_clone)
            .ok_or(shell::Error::Allocation)?;
        let mut allocations = super::declaration_store::Allocations::default();
        let pending = super::native_declarations::pending(
            owner,
            key,
            instance.try_clone().map_err(shell::Error::Status)?,
            raw,
            &mut allocations,
        )
        .map_err(shell::Error::Status)?;
        let publication = declarations
            .stage(pending, key, &mut allocations)
            .map_err(shell::Error::Status)?;
        // Every contribution at once (all or nothing): the refusals come
        // before anything is published, and the room is made first.
        self.publish_native(staging)
            .map_err(|error| shell::Error::Status(merge_status(&error)))?;
        publication.commit();
        Ok(())
    }
}

impl crate::Kernel {
    /// Publishes what a native plugin contributed: actions, screens, settings
    /// and groups. Every refusal (a duplicate, a default that does not fit, no
    /// room) comes before anything changes, and nothing allocates once
    /// publication starts, so a failure leaves no trace.
    fn publish_native(
        &mut self,
        mut staging: crate::registry::Staging,
    ) -> Result<(), RegisterError> {
        for spec in staging.settings.iter().map(crate::registry::Staged::spec) {
            if self.shared.settings.get(spec.id.as_str()).is_some() {
                return Err(duplicate(&spec.id));
            }
            if !spec.kind.accepts(&spec.default) {
                return Err(RegisterError::Invalid {
                    message: "default value does not match the setting kind".into(),
                });
            }
        }
        for group in &staging.setting_groups {
            if self
                .shared
                .settings
                .groups()
                .any(|other| other.id == group.id)
            {
                return Err(duplicate(&group.id));
            }
        }
        let (actions, settings, groups) = staging.native_counts();
        let no_room = || RegisterError::Invalid {
            message: crate::registry::NATIVE_CAPACITY_UNAVAILABLE.into(),
        };
        // A made setting needs one of the limited places: none is dropped
        // for lack of one once publication starts.
        if !self.shared.settings.has_room_for(settings) {
            return Err(no_room());
        }
        self.shared
            .actions
            .try_reserve(actions)
            .map_err(|_| no_room())?;
        self.shared
            .settings
            .try_reserve(settings, groups)
            .map_err(|_| no_room())?;
        staging.merge_into(&mut self.shared.registry, &mut self.shared.actions)?;
        for setting in staging.settings.drain(..) {
            // Checked above (no duplicate, a default that fits, a place
            // and the room); an error here would be a mistake in those.
            match setting {
                crate::registry::Staged::Owned(spec) => self.shared.settings.define(*spec),
                crate::registry::Staged::Fixed(list, at) => {
                    self.shared.settings.define_fixed(list, at)
                }
            }?;
        }
        for group in staging.setting_groups.drain(..) {
            self.shared.settings.add_group(group)?;
        }
        Ok(())
    }
}

/// `Duplicate` for `id`, or `Invalid` when the id cannot be copied.
fn duplicate(id: &Id) -> RegisterError {
    id.try_clone().map_or(
        RegisterError::Invalid {
            message: crate::registry::NATIVE_CAPACITY_UNAVAILABLE.into(),
        },
        |id| RegisterError::Duplicate { id },
    )
}

/// Admits the callback registrations of a committed batch: an event needs a
/// tag the host encodes (`ERR_UNSUPPORTED` otherwise) and one entry for each
/// tag, a draw the token of a screen or widget the plugin declared and one
/// entry for each token (`ERR_INVALID` otherwise). The registrations stay in
/// the batch, which teardown drops before the owner stops; nothing is copied.
fn check_registrations<S: Services + 'static, M: 'static>(
    retained: &Retained<S, M>,
    depth: &FlatDepth,
    tokens: &[u32],
) -> Result<(), shell::Error> {
    let mut events = super::event_tags::EventTags::default();
    let mut drawn = [0_u32; super::registrations::REGISTRATION_MAX_RECORDS];
    let mut draws = 0;
    let mut verdict = Ok(());
    retained
        .registrations(depth.as_depth(), &mut |_, record| {
            if verdict.is_err() {
                return;
            }
            verdict = match record {
                Registration::Event { event_tag, .. } => {
                    if !super::events::is_event_tag(event_tag) {
                        Err(ERR_UNSUPPORTED)
                    } else if events.contains(event_tag) {
                        Err(ERR_INVALID)
                    } else {
                        events.insert(event_tag)
                    }
                }
                Registration::Draw { token } => {
                    if !tokens.contains(&token) || drawn[..draws].contains(&token) {
                        Err(ERR_INVALID)
                    } else if draws == drawn.len() {
                        Err(paperpaw_native::host_glue::ERR_NO_SPACE)
                    } else {
                        drawn[draws] = token;
                        draws += 1;
                        Ok(())
                    }
                }
            };
        })
        .map_err(shell::Error::from)?;
    verdict.map_err(shell::Error::Status)
}

fn map_declarations(
    view: &super::declaration_checks::View<'_>,
) -> Result<(Id, Vec<Owned>, Vec<u32>), i32> {
    let owner = Id::try_parse(view.plugin).map_err(|error| match error {
        IdError::NoMemory => ERR_NO_MEMORY,
        _ => ERR_INVALID,
    })?;
    let mut owned = Vec::new();
    // The tokens of the screens and widgets, which draws may name.
    let mut tokens = Vec::new();
    let mut any = false;
    let mut result: Result<(), i32> = Ok(());
    view.visit(&mut |record| {
        if result.is_err() {
            return;
        }
        result = (|| {
            if let Record::ScreenSpec(wire::ScreenSpec { token, .. })
            | Record::WidgetSpec(wire::WidgetSpec { token, .. }) = record
            {
                tokens.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                tokens.push(token);
            }
            if let Some(value) = map_record(view.plugin, record)? {
                owned.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                owned.push(value);
            }
            any = true;
            Ok(())
        })();
    })?;
    result?;
    if !any {
        return Err(ERR_UNSUPPORTED);
    }
    Ok((owner, owned, tokens))
}

fn merge_status(error: &RegisterError) -> i32 {
    match error {
        RegisterError::Invalid { message }
            if message.as_ref() == crate::registry::NATIVE_CAPACITY_OVERFLOW
                || message.as_ref() == crate::registry::NATIVE_CAPACITY_UNAVAILABLE =>
        {
            ERR_NO_MEMORY
        }
        _ => ERR_INVALID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_native_capacity_errors_become_no_memory() {
        for message in [
            crate::registry::NATIVE_CAPACITY_OVERFLOW,
            crate::registry::NATIVE_CAPACITY_UNAVAILABLE,
        ] {
            assert_eq!(
                merge_status(&RegisterError::Invalid {
                    message: message.into()
                }),
                ERR_NO_MEMORY
            );
        }
        assert_eq!(
            merge_status(&RegisterError::Invalid {
                message: "invalid native declaration".into()
            }),
            ERR_INVALID
        );
    }
}
