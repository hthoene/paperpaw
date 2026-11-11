//! Applies what a callback staged (`apply_staged`) and tells what a native
//! screen's stage needs of the kernel (`stage_inputs`, `arm_screen_stage`).
//!
//! The one path both for the callbacks the kernel calls itself (the test
//! command runner, `shell_commands`) and for a screen's callbacks, which wait
//! in the screen queue (`screen_stage`).
use super::{
    event_call::{ScreenEffects, Staged},
    screen_stage::StageInputs,
    shell::Error,
};
use crate::Kernel;
use crate::native_host::REGISTRY_MENUS;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::Id;

impl Kernel {
    /// What the kernel alone knows of a callback of `owner` (host key
    /// `owner_key`) for its stage.
    pub(crate) fn stage_inputs(&self, owner: &str, owner_key: u64) -> StageInputs {
        let reserved = crate::commands::Reserved::of(&self.shared.settings, &self.shared.registry);
        StageInputs {
            guarded_exempt: reserved.guarded_exempt(owner),
            keys_remaining: self.screen_keys_remaining(),
            hint_buttons: !self.hint.buttons.is_empty(),
            images_held: self.image_held(owner_key),
            speech_held: self.io.speech.owned_texts(owner),
            requests_open: self.native_requests_open(),
            own_requests: self.native_requests_open_by(owner),
            answers: self.native_answer_view(owner_key),
        }
    }

    /// Lets the callbacks of the native screen at `index` build their command
    /// stage until the screen call ends ([`Self::disarm_screen_stage`]); `None`
    /// where the screen is not a running native plugin's. Armed before the
    /// call, as nothing of the kernel can be asked inside it.
    pub(crate) fn arm_screen_stage(&self, index: usize) -> Option<Option<StageInputs>> {
        let stage = self.shared.native_instances.screen_stage()?;
        let namespace = self.stack.get(index)?.id.namespace();
        let key = self.shared.native_instances.key_of(namespace)?;
        Some(stage.arm(self.stage_inputs(namespace, key)))
    }

    /// Puts back what [`Self::arm_screen_stage`] replaced.
    pub(crate) fn disarm_screen_stage(&self, armed: Option<Option<StageInputs>>) {
        if let (Some(before), Some(stage)) = (armed, self.shared.native_instances.screen_stage()) {
            stage.restore(before);
        }
    }

    /// Applies what a callback staged, once its scopes have ended: the screen
    /// policy changes first, then the commands, the picture requests, the item
    /// states, the presets, the answers to plugin requests, the labels and the widgets it said changed. One
    /// path for the callbacks the kernel calls itself and for a screen's, which
    /// wait in the screen queue.
    ///
    /// # Errors
    /// `Status(ERR_BUSY)` when the labels or widgets cannot be kept; the rest
    /// was applied.
    pub(crate) fn apply_staged(&mut self, owner: &Id, staged: Staged) -> Result<(), Error> {
        let owner_key = staged.owner;
        let ScreenEffects {
            policies,
            item_states,
            presets,
            answers,
            hints,
            widgets,
        } = staged.screen;
        self.native_screen_policies(owner, owner_key, policies);
        self.drain_native_ui(owner, staged.commands, staged.redraw);
        // The picture requests it admitted, after the callback.
        self.native_images_apply(owner_key, staged.images);
        self.apply_item_states(owner, item_states);
        let authority = self.command_settings_authority(owner);
        self.apply_native_presets(owner, authority, presets);
        self.native_apply_answers(answers);
        self.apply_labels_and_widgets(owner_key, hints, widgets)
    }

    /// The labels the callback set, and the widgets it said changed.
    fn apply_labels_and_widgets(
        &mut self,
        owner_key: u64,
        hints: alloc::vec::Vec<crate::native_host::hint_labels::HintSet>,
        widgets: alloc::vec::Vec<u32>,
    ) -> Result<(), Error> {
        // The labels the callback set; room was taken when they were checked.
        // Nothing holds the table across a callback, so a failed borrow is a
        // host fault: the callback fails (`ERR_BUSY`) rather than dropping them.
        if !hints.is_empty() {
            let Ok(mut labels) = self.shared.stack_rows.labels().try_borrow_mut() else {
                return Err(Error::Status(ERR_BUSY));
            };
            labels.apply(hints);
        }
        // The widgets it said changed: their makers hear of it after the
        // callback, from the pump (`widget_events`).
        if !widgets.is_empty()
            && let Some(ui) = self.shared.native_instances.ui_services()
            && ui.widgets_changed(owner_key, &widgets).is_err()
        {
            return Err(Error::Status(ERR_BUSY));
        }
        Ok(())
    }

    /// The states the callback pushed, applied in submit order after its
    /// other commands. The checks were made at submit; a state the host
    /// cannot store now (no memory) is dropped with a warning, the table
    /// unchanged.
    fn apply_item_states(
        &mut self,
        owner: &Id,
        (pushes, room): (
            alloc::vec::Vec<crate::native_host::item_state::ItemPush>,
            Option<crate::native_host::commands::ItemRoom>,
        ),
    ) {
        let Some(directory) = self.shared.registry.native_declarations.as_ref() else {
            return;
        };
        let (mut failed, mut changed) = (false, false);
        match directory.items.try_borrow_mut() {
            Ok(mut states) => {
                for push in pushes {
                    match states.apply(owner.as_str(), push) {
                        Ok(did) => changed |= did,
                        Err(_) => failed = true,
                    }
                }
                // A plugin that cleared its last state needs no slot (a batch
                // of defaults holds no room guard that would free it).
                states.settle(owner.as_str());
            }
            Err(_) => failed = !pushes.is_empty(),
        }
        // The room and a slot no state needs go back (nothing is borrowed).
        drop(room);
        if changed {
            // Menus show item states: bit 0 of `event.registry_changed`.
            self.native_registry_changed(REGISTRY_MENUS);
        }
        if failed {
            self.warn_runtime("item state dropped: the host could not keep it");
        }
    }
}
