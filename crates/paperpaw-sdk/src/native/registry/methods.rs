//! Typed operations over the shared registry paging implementation.
use super::*;
impl Registry {
    /// Settings in a group or plugin, or every setting for empty scope.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn settings<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistrySetting<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_SETTINGS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistrySetting::decode,
        )
    }
    /// Choices of a setting in their complete declared order.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn choices<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryChoice<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_CHOICES,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryChoice::decode,
        )
    }
    /// Child groups, or top-level groups for empty scope.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn groups<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryGroup<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_GROUPS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryGroup::decode,
        )
    }
    /// Every known plugin, with the host itself first.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn plugins<'a>(
        &self,
        host: &Host<'_>,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryPlugin<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_PLUGINS,
                scope: "",
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryPlugin::decode,
        )
    }
    /// Actions of one plugin, or all actions for empty scope.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn actions<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryItem<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_ACTIONS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryItem::decode,
        )
    }
    /// Screens of one plugin, or all screens for empty scope.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn screens<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryItem<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_SCREENS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryItem::decode,
        )
    }
    /// Slots with their current and default screens.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn slots<'a>(
        &self,
        host: &Host<'_>,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistrySlot<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_SLOTS,
                scope: "",
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistrySlot::decode,
        )
    }
    /// Screens offered for a slot, highest priority first.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn slot_offers<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryItem<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_SLOT_OFFERS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryItem::decode,
        )
    }
    /// Known menu identities.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn menus<'a>(
        &self,
        host: &Host<'_>,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryItem<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_MENUS,
                scope: "",
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryItem::decode,
        )
    }
    /// Bindings of a layer, global, or every layer for empty scope.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn bindings<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryBinding<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_BINDINGS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryBinding::decode,
        )
    }
    /// Inputs available on this device.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn inputs<'a>(
        &self,
        host: &Host<'_>,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryItem<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_INPUTS,
                scope: "",
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryItem::decode,
        )
    }
    /// Setup steps with current applicability.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn setup<'a>(
        &self,
        host: &Host<'_>,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistrySetupStep<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_SETUP,
                scope: "",
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistrySetupStep::decode,
        )
    }
    /// Presets of one kind, or every kind for empty scope.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn presets<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryItem<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_PRESETS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryItem::decode,
        )
    }
    /// Widgets of one category, or every category for empty scope.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn widgets<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryWidget<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_WIDGETS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryWidget::decode,
        )
    }
    /// The widgets that preview one settings group (`widget.spec.previews`), in
    /// the host's order: the host's own, then declarations by plugin slot, a
    /// plugin's own by name. A settings page shows the last. A group nothing
    /// previews has an empty page. `widgets` leaves these out.
    ///
    /// # Errors
    /// Returns Invalid for an empty group, codec or host errors. State
    /// requires restarting enumeration. Invalid also means the complete reply
    /// was inconsistent; no rows are exposed.
    pub fn widgets_previewing<'a>(
        &self,
        host: &Host<'_>,
        group: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryWidget<'a>>, Error> {
        if group.is_empty() {
            return Err(Error::Invalid);
        }
        self.page(
            host,
            ListQuery {
                op: schema::OP_WIDGETS,
                scope: group,
                cursor,
                flags: 1,
            },
            input,
            out,
            RegistryWidget::decode,
        )
    }
    /// Features, services and plugin requirements of a plugin.
    ///
    /// # Errors
    /// Returns codec or host errors. State requires restarting enumeration.
    /// Invalid means the complete reply was inconsistent; no rows are exposed.
    pub fn needs<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryNeed<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_NEEDS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            RegistryNeed::decode,
        )
    }
    /// Evaluated, all-available or declared-default rows of a menu.
    ///
    /// # Errors
    /// Returns codec or host errors, State for stale generations, or Invalid
    /// for an inconsistent full page. No partial page is exposed.
    pub fn menu<'a>(
        &self,
        host: &Host<'_>,
        menu: &str,
        which: MenuQuery,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryMenuRow<'a>>, Error> {
        self.page(
            host,
            ListQuery {
                op: schema::OP_MENU,
                scope: menu,
                cursor,
                flags: which.flags(),
            },
            input,
            out,
            RegistryMenuRow::decode,
        )
    }
    /// Complete values of a preset or placement, including reset and hidden flags.
    ///
    /// # Errors
    /// Returns codec or host errors, State for stale generations, or Invalid
    /// for an inconsistent full page. No partial page is exposed.
    pub fn values<'a>(
        &self,
        host: &Host<'_>,
        which: ValuesQuery<'_>,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryValue<'a>>, Error> {
        let (scope, flags) = match which {
            ValuesQuery::Preset(id) => (id, 0),
            ValuesQuery::Placement(id) => (id, 1),
        };
        self.page(
            host,
            ListQuery {
                op: schema::OP_VALUES,
                scope,
                cursor,
                flags,
            },
            input,
            out,
            RegistryValue::decode,
        )
    }
}
