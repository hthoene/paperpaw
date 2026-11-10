//! The command stage of a native screen's callbacks (`docs/abi/screens-input.md`
//! 2.1: every callback but `draw` is lent a stage).
//!
//! A screen's input and event callbacks run inside the screen's own methods,
//! where the kernel is borrowed by the router: nothing they stage can be applied
//! there. So the dispatch builds the stage ([`build_stage`]), the screen's
//! callback submits into it, and what it staged when it returned `0` (or `1` for
//! input) is kept in the **screen queue**; the kernel applies it from its pump,
//! between callbacks ([`Kernel::native_outbox_run`](crate::Kernel)). A callback
//! that returns an error drops its whole batch with the stage.
//!
//! The kernel gives the dispatch what only it knows ([`StageInputs`]) for the
//! length of one screen call (`Kernel::with_screen` arms it); a callback outside
//! a screen call has no stage to build and runs as it did.
use super::commands::{CommandAuthority, CommandStage};
use super::event_call::{ScreenEffects, Staged};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_sdk::{Env, Id, ScreenKey};

/// The batches of screen callbacks that wait for the kernel at most.
pub(crate) const SCREEN_BATCHES: usize = 4;

/// What a stage needs that only the kernel knows, at the time of a screen call.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StageInputs {
    /// The plugin may change the guarded settings (it is the settings editor).
    pub(crate) guarded_exempt: bool,
    /// How many screen keys the kernel can still hand out.
    pub(crate) keys_remaining: u32,
    /// The device shows a row of button hints.
    pub(crate) hint_buttons: bool,
    /// The plugin's picture requests that wait outside the table.
    pub(crate) images_held: usize,
}

/// The device-wide hand-off between the kernel and the screens' dispatch.
#[derive(Default)]
pub(crate) struct ScreenStage {
    inputs: Cell<Option<StageInputs>>,
    queue: RefCell<Vec<Staged>>,
}

impl ScreenStage {
    pub(crate) const fn new() -> Self {
        Self {
            inputs: Cell::new(None),
            queue: RefCell::new(Vec::new()),
        }
    }

    /// Arms `inputs` for a screen call; the result is what was armed before
    /// (a screen call inside another), to be put back by [`Self::restore`].
    pub(crate) fn arm(&self, inputs: StageInputs) -> Option<StageInputs> {
        self.inputs.replace(Some(inputs))
    }

    pub(crate) fn restore(&self, before: Option<StageInputs>) {
        self.inputs.set(before);
    }

    pub(crate) fn inputs(&self) -> Option<StageInputs> {
        self.inputs.get()
    }

    /// Whether another batch can wait.
    pub(crate) fn has_room(&self) -> bool {
        self.queue
            .try_borrow()
            .is_ok_and(|queue| queue.len() < SCREEN_BATCHES)
    }

    /// Keeps a batch for the kernel.
    ///
    /// # Errors
    /// `ERR_BUSY` when the queue is full, `ERR_NO_MEMORY` when it cannot grow.
    pub(crate) fn put(&self, staged: Staged) -> Result<(), i32> {
        let mut queue = self.queue.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if queue.len() >= SCREEN_BATCHES {
            return Err(ERR_BUSY);
        }
        queue.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        queue.push(staged);
        Ok(())
    }

    /// The oldest batch.
    pub(crate) fn take(&self) -> Option<Staged> {
        let mut queue = self.queue.try_borrow_mut().ok()?;
        (!queue.is_empty()).then(|| queue.remove(0))
    }
}

/// What a stage is built from: the one list for the kernel's own call and
/// for a screen's dispatch.
pub(crate) struct StageParts<'a> {
    pub(crate) owner: &'a Id,
    pub(crate) owner_key: u64,
    pub(crate) env: &'a Env<'a>,
    /// The screen instance the callback speaks for, and how many commands
    /// deep the chain it runs in is.
    pub(crate) screen: Option<ScreenKey>,
    pub(crate) hop: u8,
    pub(crate) inputs: StageInputs,
    pub(crate) stack_rows: &'a super::stack_rows::StackRows,
    pub(crate) ui: Option<&'a super::ui_service::UiServices>,
    pub(crate) directory: Option<&'a super::native_declarations::DirectoryHandle>,
}

/// The command stage of one callback of `parts.owner`.
pub(crate) fn build_stage<'a>(parts: StageParts<'a>) -> CommandStage<'a> {
    let StageParts {
        owner,
        owner_key,
        env,
        screen,
        hop,
        inputs,
        stack_rows,
        ui,
        directory,
    } = parts;
    let mut stage = CommandStage::new(
        owner,
        env.settings(),
        Some(CommandAuthority::new(inputs.guarded_exempt)),
        super::commands::CALLBACK_COMMANDS,
        super::commands::CALLBACK_COMMAND_BYTES,
    )
    .with_ui(env, screen, hop, inputs.keys_remaining)
    .with_stack(stack_rows, owner_key)
    .with_hint_labels(stack_rows.labels(), inputs.hint_buttons)
    .with_widgets(ui, owner_key);
    stage.set_images_held(inputs.images_held);
    if let Some(directory) = directory {
        stage = stage.with_item_states(super::commands::ItemAdmit { directory });
    }
    stage
}

/// What the callback staged, after it returned `0` or `1`: the stage consumed
/// into a batch for the kernel.
pub(crate) fn staged_of(mut stage: CommandStage<'_>, owner_key: u64) -> Staged {
    let policies = stage.take_policies();
    let item_states = stage.take_item_states();
    let presets = stage.take_presets();
    let images = stage.take_image_commands();
    let hints = stage.take_hint_sets();
    let widgets = stage.take_widget_changes();
    let (commands, redraw, _hop) = stage.into_ui_output();
    let mut staged = Staged::new(owner_key, commands, redraw, images);
    staged.screen = ScreenEffects {
        policies,
        item_states,
        presets,
        hints,
        widgets,
    };
    staged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(owner: u64) -> Staged {
        Staged::new(owner, Vec::new(), None, Vec::new())
    }

    #[test]
    fn the_queue_keeps_four_batches_in_order_and_refuses_a_fifth() {
        let queue = ScreenStage::new();
        for owner in 1..=4 {
            assert!(queue.has_room());
            queue.put(batch(owner)).unwrap();
        }
        assert!(!queue.has_room());
        assert_eq!(queue.put(batch(5)), Err(ERR_BUSY));
        for owner in 1..=4 {
            assert_eq!(queue.take().map(|staged| staged.owner), Some(owner));
        }
        assert!(queue.take().is_none());
        assert!(queue.has_room());
    }

    #[test]
    fn arming_nests_and_restores_what_was_armed_before() {
        let queue = ScreenStage::new();
        let inputs = |keys| StageInputs {
            guarded_exempt: false,
            keys_remaining: keys,
            hint_buttons: false,
            images_held: 0,
        };
        assert!(queue.inputs().is_none());
        let outer = queue.arm(inputs(1));
        let inner = queue.arm(inputs(2));
        assert_eq!(queue.inputs().map(|i| i.keys_remaining), Some(2));
        queue.restore(inner);
        assert_eq!(queue.inputs().map(|i| i.keys_remaining), Some(1));
        queue.restore(outer);
        assert!(queue.inputs().is_none());
    }
}
