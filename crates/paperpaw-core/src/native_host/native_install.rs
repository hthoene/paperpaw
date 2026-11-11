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
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE, ERR_UNSUPPORTED, Services,
};
use paperpaw_sdk::{Contribution, Id, IdError, RegisterError};

/// Owned mapping results; future declaration families extend this one dispatch.
enum Owned {
    Screen(Callback),
    Action(Callback),
    /// Boxed at once: a spec is large, and the list holds one place for each
    /// declaration (the registration peak on the X4).
    Setting(alloc::boxed::Box<paperpaw_sdk::SettingSpec>),
    /// The settings equal to the entries `.1` of the plugin's table `.0` in the
    /// program, in the table's order: no copy of them is kept
    /// (`fixed_settings`). One place for all of them, not one of a spec's size
    /// for each.
    FixedSettings(&'static [paperpaw_sdk::SettingSpec], Vec<u16>),
    Group(alloc::boxed::Box<paperpaw_sdk::SettingGroupSpec>),
    Resource(super::native_resources::Resource),
    /// A screen the plugin offers for a slot (`slot.offer`).
    SlotOffer {
        slot: Id,
        screen: Id,
        priority: paperpaw_sdk::Priority,
    },
}

fn map_record(owner: &str, record: Record<'_>) -> Result<Option<Owned>, i32> {
    match record {
        Record::ScreenSpec(_)
        | Record::ActionSpec(_)
        | Record::SettingSpec(_)
        | Record::SettingGroup(_) => match native_values::from_record(owner, record)? {
            Value::Screen(callback) => Ok(Some(Owned::Screen(callback))),
            Value::Action(callback) => Ok(Some(Owned::Action(callback))),
            Value::Setting(spec) => Ok(Some(Owned::Setting(
                super::shared::try_box(spec).ok_or(ERR_NO_MEMORY)?,
            ))),
            Value::Group(group) => Ok(Some(Owned::Group(
                super::shared::try_box(group).ok_or(ERR_NO_MEMORY)?,
            ))),
        },
        // The extras (section, editor, check) are presentation the settings
        // page reads from the declaration store; the store keeps the record.
        Record::SettingExtra(_) => Ok(None),
        // A stage and a subscription are read from the declaration store (the
        // broker is fed by `DeclarationStore::stage` and `stage_subscribers`);
        // they go with the owner's declarations.
        Record::StageSpec(_) | Record::RegistrationStage(_) => Ok(None),
        // A setup step stays in the declaration store; the registry's `setup`
        // query evaluates it where it is read (`registry.md` section 6).
        Record::SetupStep(_) => Ok(None),
        Record::ResourceFontFace(_)
        | Record::ResourceFontExtension(_)
        | Record::ResourceHyphenation(_) => {
            Ok(super::native_resources::map(owner, record)?.map(Owned::Resource))
        }
        Record::WidgetSpec(_)
        | Record::WidgetPlace(_)
        | Record::StatusItem(_)
        | Record::StatusLine(_)
        | Record::StatusPlace(_) => Ok(None),
        // A slot the plugin names stays in the declaration store (its title is
        // read from there); an offer also goes to the registry the kernel
        // opens slots by (`Registry::resolve_slot`). `Staging::native_offer`
        // refuses an offer of a screen that is not the plugin's own. The
        // contract gives the priority no range (an i32), the kernel's is an
        // i16: one outside it counts as the nearest end, so an offer that
        // declared fine never fails the install.
        Record::SlotSpec(_) => Ok(None),
        Record::SlotOffer(offer) => Ok(Some(Owned::SlotOffer {
            slot: native_values::checked_id(offer.slot)?,
            screen: native_values::checked_id(offer.screen)?,
            priority: paperpaw_sdk::Priority(i16::try_from(offer.priority).unwrap_or(
                if offer.priority < 0 {
                    i16::MIN
                } else {
                    i16::MAX
                },
            )),
        })),
        // Menus and icons stay in the declaration store (charged there, removed
        // with their owner); `native_menus` reads them.
        Record::MenuEntry(_)
        | Record::IconSpec(_)
        | Record::MenuNeed(_)
        | Record::MenuSection(_)
        | Record::MenuHide(_)
        | Record::MenuDescribe(_) => Ok(None),
        // A key binding stays in the declaration store too (charged there,
        // removed with its owner); the key map is built from it
        // (`native_keys`). One the key map cannot hold is refused here, never
        // dropped later.
        Record::KeyBind(bind) => {
            if crate::keymap::native_binding_ok(bind.layer, bind.input, bind.action) {
                Ok(None)
            } else {
                Err(ERR_INVALID)
            }
        }
        // An input of the plugin's own and a help hint stay in the declaration
        // store (`registry.md` op 14 reads the first; `ui.md` op 3 the second).
        Record::InputSpec(_) | Record::HelpHint(_) => Ok(None),
        // A format, a reading app and a lookup provider stay in the
        // declaration store too (`document.md` sections 1 and 7); the
        // document and lookup services read them there.
        Record::FormatSpec(_) | Record::ReaderSpec(_) | Record::LookupSpec(_) => Ok(None),
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
        self.register_native_in(host, depth, None, report)
    }

    /// [`Self::register_native`] for a linked plugin that keeps its settings
    /// in `fixed`, a table in the program (`Kernel::register_linked`): a setting
    /// it declares that equals an entry of the table is kept as that entry and
    /// not as a copy on the heap.
    pub(crate) fn register_native_fixed<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        report: fn(&S, u32, i32),
        fixed: &'static [paperpaw_sdk::SettingSpec],
    ) -> Result<(), shell::Error> {
        self.register_native_with(host, depth, None, report, fixed)
    }

    /// [`Self::register_native`] for a plugin that lives in `folder` (the loader
    /// of its `.ppn` knows it): the font and hyphenation files its records name
    /// are relative to it. A plugin with such records and no folder (a linked
    /// built-in) is refused whole with `ERR_UNSUPPORTED`; so is one with an
    /// empty folder (a folder written with a trailing `/` is the same folder).
    ///
    /// When it brings fonts the choices of reading font and status font are
    /// offered anew and the fonts chosen applied, as `plugins_changed` does.
    /// The refresh never aborts on a short heap: where it cannot be made the
    /// plugin stays installed, the choices and fonts in use stay and a
    /// warning says so. It may stage a command (the font size, if the reading
    /// font has none so large): the next round of commands runs it, so the
    /// caller does nothing about it.
    ///
    /// # Errors
    /// As [`Self::register_native`].
    pub fn register_native_in<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        folder: Option<&str>,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        self.register_native_with(host, depth, folder, report, &[])
    }

    fn register_native_with<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        folder: Option<&str>,
        report: fn(&S, u32, i32),
        fixed: &'static [paperpaw_sdk::SettingSpec],
    ) -> Result<(), shell::Error> {
        let fonts_before = self.shared.fonts.ids().count();
        let offers_before = self.shared.registry.offers();
        let keys_before = self.native_key_count();
        self.admit_native_with(host, depth, folder, report, fixed)?;
        if self.native_key_count() != keys_before {
            // Its bindings are in the key map from now on.
            self.rebuild_keymap();
        }
        if self.shared.registry.offers() != offers_before {
            // The slots now bound are known to plugins (`Env::has_slot`).
            self.refresh_slots();
        }
        if self.shared.fonts.ids().count() != fonts_before {
            // The families it brought are offered, and a font chosen that was
            // waiting for it is in use. This is the kernel's own refresh, the
            // one `plugins_changed` runs for every plugin and the builder at
            // start-up; it is not part of the admission, whose allocation
            // faults the tests sweep ([`Self::admit_native`]).
            self.offer_font_choices();
            self.apply_reading_font();
            self.apply_status_font();
        }
        Ok(())
    }

    /// The admission of [`Self::register_native_in`], without the refresh of
    /// the font choices: all or nothing, and every allocation fallible.
    #[cfg(test)]
    pub(crate) fn admit_native<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        folder: Option<&str>,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        self.admit_native_with(host, depth, folder, report, &[])
    }

    /// [`Self::admit_native`] for a plugin with a table of settings kept in the
    /// program (`fixed`, empty for none).
    fn admit_native_with<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        folder: Option<&str>,
        report: fn(&S, u32, i32),
        fixed: &'static [paperpaw_sdk::SettingSpec],
    ) -> Result<(), shell::Error> {
        let networked = host.features().feature(crate::work::USES_NETWORK).is_ok();
        self.shared
            .native_instances
            .adopt_pool(
                host.resource_identity()
                    .ok_or(shell::Error::Status(ERR_NO_MEMORY))?,
            )
            .map_err(shell::Error::Status)?;
        self.shared
            .native_instances
            .attach_screens(&self.shared.stack_rows);
        if self.shared.registry.native_declarations.is_none() {
            self.shared.registry.native_declarations = Some(
                super::shared::Shared::try_new(
                    super::native_declarations::NativeDeclarations::default(),
                )
                .map_err(|_| shell::Error::Allocation)?,
            );
        }
        let depth = depth.try_clone().ok_or(shell::Error::Allocation)?;
        // The first plugin's depth is the shell's device-wide one, and every
        // later plugin brings the same: a host stage run enters it, so a
        // second counter would let nested dispatch past the budget.
        match self.shared.native_depth.as_ref() {
            None => {
                self.shared.native_depth = Some(depth.try_clone().ok_or(shell::Error::Allocation)?);
            }
            Some(first) if !first.is_same_counter(&depth) => {
                self.warn_runtime("a native plugin brought its own callback depth counter");
                return Err(shell::Error::Status(ERR_STATE));
            }
            Some(_) => {}
        }
        let folder = folder
            .map(|folder| folder.trim_end_matches('/'))
            .filter(|folder| !folder.is_empty());
        // A batch refused at a declaration cap keeps which one, for the
        // plugin's `reg_plugin` row.
        let refused = self.shared.registry.native_declarations.as_ref();
        let (retained, (owner, owned, tokens, declared), raw) = Retained::make_native_stored(
            host,
            |view| map_declarations(view, fixed),
            |plugin, cap| {
                if let Some(refused) = refused {
                    refused.record_refusal(plugin, cap);
                }
            },
        )?;
        self.warn_unmatched(fixed, &owned);
        if folder.is_none()
            && owned
                .iter()
                .any(|value| matches!(value, Owned::Resource(_)))
        {
            self.warn_runtime("a plugin with font or hyphenation files needs its folder");
            return Err(shell::Error::Status(ERR_UNSUPPORTED));
        }
        // The callback registrations are admitted whole or the owner is not:
        // none is dropped unrouted.
        let events = check_registrations(&retained, &depth, &tokens)?;
        // Admitted when it declares OR registers anything (an events-only
        // plugin is as good as any); a library with nothing at all is no
        // plugin: invalid, not "needs a newer host".
        if !declared && events.is_empty() {
            // Refused before the kernel takes it in. An empty library has run
            // only `make`, so its cleanup cannot be busy; say so if that
            // ever stops being true (the owner would stay pinned).
            let pool = retained.host().resource_identity();
            drop(retained);
            if pool.is_some_and(|pool| pool.retirements_pending()) {
                self.try_warn_runtime(&[
                    "a library with nothing in it could not be cleaned up; retried later",
                ]);
            }
            return Err(shell::Error::Status(ERR_INVALID));
        }
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
        let result = self.bind_native(
            key,
            retained,
            depth,
            (owned, raw, events, folder.unwrap_or_default()),
            &owner,
            report,
        );
        if result.is_err() {
            self.shared
                .native_instances
                .abandon(key)
                .map_err(shell::Error::Status)?;
        } else {
            // With `uses.network` its network settings and policy (never a
            // refusal: a failure warns).
            self.admit_native_network(&owner, networked);
            // What a power cut left of it in the services is cleaned up: only
            // for a plugin that can use them (`uses.network` covers `core.net`
            // and the sockets), so a plain plugin's start costs no card lookup.
            if networked {
                self.native_services_owner_started(key);
            }
        }
        result
    }

    /// The slots some screen is bound to, as plugins read them
    /// ([`paperpaw_sdk::Env::has_slot`]), made anew where offers came or went:
    /// where the heap has no room for the list the one known stays, and a
    /// warning says so.
    fn refresh_slots(&mut self) {
        match self.shared.registry.try_bound_slots() {
            Some(slots) => self.shared.slots = slots,
            None => {
                // The list known stays, less the slots whose last offer went.
                self.shared.registry.retain_bound(&mut self.shared.slots);
                self.try_warn_runtime(&[
                    "not enough memory to update the slots plugins offer screens for; the list stays, less slots whose last offer went",
                ]);
            }
        }
    }

    /// Revoke first; store cleanup, screen closure and factory removal must
    /// precede resource retirement. Busy cleanup retains a pending row.
    pub(crate) fn begin_native_unload(
        &mut self,
        key: u64,
        owner: &Id,
    ) -> Option<alloc::string::String> {
        // Counted before the stop: a revoked owner's rows no longer show.
        let keys_before = self.native_key_count();
        if self.shared.native_instances.stop(key).is_err() {
            return None;
        }
        // Its firmware requests and its confirmation go with it.
        self.firmware_sender_stopped(key);
        let _bump = crate::native_host::registry_generation::BumpOnExit::new(
            &self.shared.registry_generation,
        );
        self.warn_directory_missed();
        let mut forgot_states = false;
        if let Some(declarations) = &self.shared.registry.native_declarations {
            let Ok(mut store) = declarations.store.try_borrow_mut() else {
                return None;
            };
            store.remove_host_owner(key);
            if declarations.catalog.prune(&store).is_err() {
                return None;
            }
            // Its pushed item states go with it (`keep` waits for the kept
            // file). The table is only borrowed here, never across a call.
            let Ok(mut states) = declarations.items.try_borrow_mut() else {
                return None;
            };
            forgot_states = states.forget(owner.as_str());
        }
        if forgot_states {
            // The menus lose the states the plugin pushed (bit 0).
            self.native_registry_changed(1);
        }
        if self.native_key_count() != keys_before {
            // Its bindings leave the key map with it.
            self.rebuild_keymap();
        }
        if self.native_owner_stopped(key).is_err() {
            return None;
        }
        self.remove_retired_native_screens(owner);
        self.shared
            .registry
            .screens
            .remove_native_owner(owner.as_str());
        // Its offers for slots go with its screens.
        self.shared.registry.remove_native_offers(owner.as_str());
        self.refresh_slots();
        // The actions hold the plugin's lease; the settings and groups it
        // defined go with it (the reader's values stay for its next load).
        self.shared.actions.remove_plugin(owner.as_str());
        self.shared.settings.remove_plugin(owner.as_str());
        // Its network policy goes; its network settings stay for its next load.
        self.forget_native_network(owner.as_str());
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
        let mut result = Ok(());
        if let Some(store) = self.shared.native_instances.ui_store() {
            result = (|| {
                store.dialog_closed(instance)?;
                store.asker_instance_closed(instance, &mut |cancelled| {
                    if let Some(dialog) = cancelled.dialog {
                        self.queue_native_dialog_close(dialog);
                    }
                })?;
                store.instance_closed(instance)
            })();
        }
        // The widget instances the screen made go too, whatever the store
        // steps said (a retry repeats every step; each is idempotent).
        if let Some(ui) = self.shared.native_instances.ui_services() {
            let widgets = ui.screen_closed(instance).map(|_| ());
            result = result.and(widgets);
        }
        // The servers and sockets the screen started end with it.
        result.and(self.native_services_screen_closed(instance))
    }

    /// An owner-directory update found the table held and was skipped: said
    /// once per batch (a skipped subscription fails closed).
    fn warn_directory_missed(&mut self) {
        if self.shared.native_instances.take_directory_missed() > 0 {
            self.warn_runtime("owner directory update skipped: the table was in use");
        }
    }

    pub(crate) fn retry_native_unloads(&mut self) {
        self.warn_directory_missed();
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
        mapped: (
            Vec<Owned>,
            super::raw_declarations::RawDeclarations,
            super::event_tags::EventTags,
            &str,
        ),
        owner: &Id,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        let (owned, raw, events, folder) = mapped;
        let mut flat_host = self.flat_host();
        flat_host.directory = self
            .shared
            .registry
            .native_declarations
            .as_ref()
            .map(|directory| directory.downgrade().ok_or(shell::Error::Allocation))
            .transpose()?;
        let instance = NativeLease::try_new(NativeDispatch::with_events(
            retained,
            depth,
            flat_host,
            report,
            self.shared
                .native_instances
                .identity(key)
                .map_err(shell::Error::Status)?,
            events,
        ))
        .map_err(shell::Error::Status)?;
        let mut staging = crate::registry::Staging::default();
        let mut resources = super::native_resources::Resources::default();
        for value in owned {
            match value {
                Owned::Resource(resource) => resources
                    .add(resource, folder)
                    .map_err(shell::Error::Status)?,
                Owned::Screen(callback) => {
                    let screen = instance
                        .try_clone()
                        .and_then(|lease| NativeScreenSpec::new(callback, lease))
                        .map_err(shell::Error::Status)?;
                    staging
                        .native_screen(owner, screen)
                        .map_err(shell::Error::Status)?;
                }
                Owned::SlotOffer {
                    slot,
                    screen,
                    priority,
                } => staging
                    .native_offer(owner, slot, screen, priority)
                    .map_err(shell::Error::Status)?,
                Owned::Action(callback) => {
                    let action = instance
                        .try_clone()
                        .and_then(|lease| super::native_action::spec(callback, lease))
                        .map_err(shell::Error::Status)?;
                    let action = boxed(action)?;
                    staging
                        .native(owner, Contribution::Action(action))
                        .map_err(shell::Error::Status)?;
                }
                Owned::Setting(spec) => {
                    staging
                        .native(owner, Contribution::Setting(spec))
                        .map_err(shell::Error::Status)?;
                }
                Owned::FixedSettings(list, entries) => staging
                    .native_fixed_entries(list, &entries)
                    .map_err(shell::Error::Status)?,
                Owned::Group(group) => {
                    let group = paperpaw_sdk::SettingGroup::from_boxed(group);
                    staging
                        .native(owner, Contribution::SettingGroup(group))
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
        self.publish_native(staging, owner, folder, resources)
            .map_err(|error| shell::Error::Status(merge_status(&error)))?;
        publication.commit();
        Ok(())
    }
}

impl crate::Kernel {
    /// A table that does not equal what the plugin declares costs the memory it
    /// should save: said in debug builds, where it is mended.
    #[cfg(debug_assertions)]
    fn warn_unmatched(&mut self, fixed: &[paperpaw_sdk::SettingSpec], owned: &[Owned]) {
        if !fixed.is_empty() && owned.iter().any(|value| matches!(value, Owned::Setting(_))) {
            self.warn_runtime(
                "a linked plugin's settings table does not match some of its settings; they are copied",
            );
        }
    }

    #[cfg(not(debug_assertions))]
    #[allow(clippy::unused_self, reason = "the debug build says it")]
    fn warn_unmatched(&mut self, _fixed: &[paperpaw_sdk::SettingSpec], _owned: &[Owned]) {}
}

impl crate::Kernel {
    /// Publishes what a native plugin contributed: actions, screens, settings
    /// and groups. Every refusal (a duplicate, a default that does not fit, no
    /// room) comes before anything changes, and nothing allocates once
    /// publication starts, so a failure leaves no trace.
    /// The shell has hosted its native plugins: the registries and the
    /// tables of the host give back the room they kept while the plugins were
    /// added one by one. The build shrank them before any native plugin was
    /// there, so what they grew by since (the plugin list, the menus, the
    /// subscriptions, the entries and owner rows) stood in reserve for good.
    /// Called once by [`Kernel::start`](crate::Kernel::start); a later install
    /// grows them again, as it always did.
    pub(crate) fn native_hosted(&mut self) {
        self.shrink_to_fit();
        self.shared.native_instances.shrink_to_fit();
    }

    fn publish_native(
        &mut self,
        staging: crate::registry::Staging,
        owner: &Id,
        folder: &str,
        resources: super::native_resources::Resources,
    ) -> Result<(), RegisterError> {
        self.check_resources(&resources)?;
        let _bump = crate::native_host::registry_generation::BumpOnExit::new(
            &self.shared.registry_generation,
        );
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
        let fixed = staging.native_fixed_count();
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
            .try_reserve_with_fixed(settings, fixed, groups)
            .map_err(|_| no_room())?;
        self.reserve_resources(owner, folder, &resources)?;
        // Whatever refuses from here on takes the hyphenation tables out
        // again (nothing else is published before the merge).
        let published = self.publish_rest(staging, owner, resources);
        if published.is_err() {
            self.undo_resources(owner);
        }
        published
    }

    /// The part of the publication after the room is made.
    fn publish_rest(
        &mut self,
        mut staging: crate::registry::Staging,
        owner: &Id,
        resources: super::native_resources::Resources,
    ) -> Result<(), RegisterError> {
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
        // The device's defaults for what the plugin has declared (the kernel
        // was built before it, so they were not known then).
        self.apply_late_defaults();
        self.publish_fonts(owner, resources)
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
/// tag, a draw the token of exactly one screen, widget, status item or status line
/// the plugin declared and one entry for each token (`ERR_INVALID` otherwise). The registrations stay in
/// the batch, which teardown drops before the owner stops. The admitted event
/// tags are returned, for the instance to deliver by.
fn check_registrations<S: Services + 'static, M: 'static>(
    retained: &Retained<S, M>,
    depth: &FlatDepth,
    tokens: &[u32],
) -> Result<super::event_tags::EventTags, shell::Error> {
    let mut events = super::event_tags::EventTags::default();
    let mut drawn: Vec<u32> = Vec::new();
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
                    // The token of exactly one drawable declaration, named once.
                    if tokens.iter().filter(|&&other| other == token).count() != 1
                        || drawn.contains(&token)
                    {
                        Err(ERR_INVALID)
                    } else if drawn.try_reserve(1).is_err() {
                        Err(ERR_NO_MEMORY)
                    } else {
                        drawn.push(token);
                        Ok(())
                    }
                }
            };
        })
        .map_err(shell::Error::from)?;
    verdict.map_err(shell::Error::Status)?;
    Ok(events)
}

fn map_declarations(
    view: &super::declaration_checks::View<'_>,
    fixed: &'static [paperpaw_sdk::SettingSpec],
) -> Result<(Id, Vec<Owned>, Vec<u32>, bool), i32> {
    // Entries are numbered in two bytes; a longer table is not used.
    let fixed = if fixed.len() > usize::from(u16::MAX) {
        &[]
    } else {
        fixed
    };
    // The host's own id is reserved: no plugin's action passes for a host
    // one (ui.md 4.3).
    if view.plugin == crate::RESERVED_PLUGIN_ID {
        return Err(ERR_DENIED);
    }
    let owner = Id::try_parse(view.plugin).map_err(|error| match error {
        IdError::NoMemory => ERR_NO_MEMORY,
        _ => ERR_INVALID,
    })?;
    let mut owned = Vec::new();
    // Room for what will be listed, once: growing by doubling held both
    // the old and the new list at the registration peak. A setting that a
    // table in the program stands for takes a place only for a run of them.
    let mut upcoming = usize::from(!fixed.is_empty());
    let mut declared_settings = 0;
    view.visit(&mut |record| match record {
        Record::SettingSpec(_) if !fixed.is_empty() => declared_settings += 1,
        Record::ScreenSpec(_)
        | Record::ActionSpec(_)
        | Record::SettingSpec(_)
        | Record::SettingGroup(_)
        | Record::ResourceFontFace(_)
        | Record::ResourceFontExtension(_)
        | Record::ResourceHyphenation(_)
        | Record::SlotOffer(_) => upcoming += 1,
        _ => {}
    })?;
    owned
        .try_reserve_exact(upcoming)
        .map_err(|_| ERR_NO_MEMORY)?;
    // The tokens of what the plugin draws itself (screens, widgets, status
    // items and status lines), which draws may name.
    let mut tokens = Vec::new();
    let mut any = false;
    // The table entries the plugin's settings stand for.
    let mut entries: Vec<u16> = Vec::new();
    entries
        .try_reserve_exact(fixed.len().min(declared_settings))
        .map_err(|_| ERR_NO_MEMORY)?;
    let mut result: Result<(), i32> = Ok(());
    view.visit(&mut |record| {
        if result.is_err() {
            return;
        }
        result = (|| {
            if let Record::ScreenSpec(wire::ScreenSpec { token, .. })
            | Record::WidgetSpec(wire::WidgetSpec { token, .. })
            | Record::StatusItem(wire::StatusItem { token, .. })
            | Record::StatusLine(wire::StatusLine { token, .. }) = record
            {
                tokens.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                tokens.push(token);
            }
            if let Some(value) = map_record(view.plugin, record)? {
                if let Owned::Setting(spec) = &value
                    && let Some(at) = super::fixed_settings::find(fixed, spec)
                {
                    // Equal to an entry of the plugin's table: that entry is
                    // kept, and the copy made here goes at once. The records
                    // come in no order the table keeps; the entries are put
                    // in the table's order below. A setting that matches
                    // none stays a copy and is published before them.
                    entries.push(u16::try_from(at).map_err(|_| ERR_INVALID)?);
                    return Ok(());
                }
                owned.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                owned.push(value);
            }
            any = true;
            Ok(())
        })();
    })?;
    result?;
    if !entries.is_empty() {
        // In the table's order, which is the order they were declared in.
        entries.sort_unstable();
        owned.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        owned.push(Owned::FixedSettings(fixed, entries));
    }
    // Whether it declared anything is told with the result: a library that only
    // registers callbacks (an event, a tick) is admitted too; one with neither
    // is refused where the registrations are known ([`Kernel::admit_native`]).
    Ok((owner, owned, tokens, any))
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

/// One owned contribution in a fallible box: no room is an error, never an
/// abort.
fn boxed<T>(value: T) -> Result<alloc::boxed::Box<T>, shell::Error> {
    super::shared::try_box(value).ok_or(shell::Error::Status(ERR_NO_MEMORY))
}

/// How many settings `fixed` (a plugin's table) takes in place of a copy, and
/// how many stay owned copies, for the declarations in `view`.
#[cfg(test)]
pub(super) fn settings_paths(
    view: &super::declaration_checks::View<'_>,
    fixed: &'static [paperpaw_sdk::SettingSpec],
) -> Result<(usize, usize), i32> {
    let (_, owned, _, _) = map_declarations(view, fixed)?;
    let kept = owned
        .iter()
        .map(|value| match value {
            Owned::FixedSettings(_, entries) => entries.len(),
            _ => 0,
        })
        .sum();
    let copied = owned
        .iter()
        .filter(|value| matches!(value, Owned::Setting(_)))
        .count();
    Ok((kept, copied))
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
