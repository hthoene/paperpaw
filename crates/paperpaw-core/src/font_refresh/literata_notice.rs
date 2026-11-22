//! The upgrade hint for a saved Literata choice on a card without its pack.
use crate::Ask;
use paperpaw_sdk::{Id, Params, builtin::slots};

pub(crate) const SEEN: &str = "literata-pack-notice-seen";

#[derive(Default)]
pub(crate) struct LiterataNotice {
    pub(crate) pending: bool,
    pub(crate) seen: bool,
}

impl crate::Kernel {
    pub(crate) fn literata_notice_answer(&mut self, params: &Params) {
        self.save_config("core", SEEN, "on");
        if params.get("store") == Some("yes") {
            self.output.commands.push(Ask::OpenScreen(
                Id::new_static("store.plugin"),
                Params::new().with("id", "literata-styles"),
            ));
        }
    }

    /// Whether a start notice (this hint, the one of passwords that do not
    /// open) may show now: none showed since the start, and the home screen is
    /// on top alone and settled (nothing waits to be shown or done).
    pub(crate) fn start_notice_may_show(&self) -> bool {
        let home = self.stack.len() == 1
            && self
                .stack
                .last()
                .is_some_and(|s| s.slot.as_ref() == Some(&slots::HOME));
        home && !self.waiting_notices.start_notice_shown
            && !self.show_pending
            && self.output.commands.is_empty()
            && self.service_due().is_none_or(|at| at > self.shared.now)
    }

    /// Only the settled Home screen may receive the upgrade hint, never Setup
    /// or a reader that is reopening its book. This is not called from input.
    #[inline(never)]
    pub(crate) fn offer_literata_notice(&mut self) -> bool {
        if !self.waiting_notices.literata.pending
            || self.waiting_notices.literata.seen
            || !self.start_notice_may_show()
        {
            return false;
        }
        if self
            .shared
            .fonts
            .family_there("Literata", self.shared.storage.as_ref())
        {
            self.waiting_notices.literata.pending = false;
            return false;
        }
        let store = self.shared.caps.wifi
            && self
                .shared
                .secrets
                .iter()
                .any(|s| s.id.starts_with(paperpaw_sdk::wifi::SECRET_PREFIX))
            && self.shared.registry.screens.contains_key("store.plugin");
        self.shared
            .registry
            .screens
            .insert_fixed(&crate::host_screen::LITERATA);
        self.open(
            crate::native_host::literata_notice::SCREEN,
            None,
            &Params::new().with("store", if store { "yes" } else { "no" }),
        );
        self.waiting_notices.literata.seen = true;
        self.waiting_notices.start_notice_shown = true;
        self.waiting_notices.literata.pending = false;
        true
    }
}
