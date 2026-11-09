//! The host's own choice and confirm dialogs, answering a `core.ui` question
//! (`docs/abi/ui.md` 3.1 and 3.2) when no dialog plugin serves the slot: the
//! core fallbacks of `core.choice` and `core.confirm` are interface 6 screens
//! that answer with a reply event, so the kernel wraps them. A [`HostDialog`]
//! runs the inner screen against a private output and turns its reply into the
//! answer of the request through the store's one answer path (the same as
//! op 11: only the instance the host opened, the first final answer wins).
//! The reply event itself never leaves the wrapper: no plugin and no held-event
//! store hears it. *Back*, and a dialog closed without an answer, cancel the
//! request (the stack's close tells the store).
//!
//! Kinds 0 (text) and 3 (folder) have no host dialog: without a dialog plugin
//! they end as cancelled, as the pump leaves them.
use super::{shared::Shared, ui_store::UiStore};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, Handled, Hint, Id, Output, Params, Rect, Screen,
    builtin::params,
    ui::confirm::{ANSWER, FOCUS_YES, NO_LABEL, ONLY_YES, YES_LABEL},
};

/// The private event the inner screen replies with: caught by the wrapper.
const REPLY: &str = "core.host-dialog";
/// The options key of the inner choice screen, a comma separated list.
const OPTIONS: &str = "options";
/// The option the inner choice screen replies with.
const OPTION: &str = "option";
/// A comma inside a line of a choice would split it in two: this stands in for
/// it (a low quotation mark, which looks like one).
const COMMA: char = '\u{201a}';

/// The question as interface 6 params for the inner screen of `kind` (1 choice,
/// 2 confirm); `None` for any other kind. Gives the options of a choice too,
/// in the order the screen lists them.
pub(super) fn inner_params(
    kind: u32,
    flags: u32,
    title: &str,
    text: &str,
    choices: &str,
) -> Option<(Params, Vec<String>)> {
    let base = Params::new()
        .with(params::TITLE, title.to_string())
        .with(params::TEXT, text.to_string())
        .with(params::REPLY, REPLY);
    match kind {
        1 => {
            let options: Vec<String> = choices
                .split('\n')
                .map(|line| line.replace(',', &COMMA.to_string()))
                .collect();
            let joined = options.join(",");
            Some((base.with(OPTIONS, joined), options))
        }
        2 => {
            let mut params = base;
            let mut words = choices.split('\n');
            if let Some(yes) = words.next().filter(|w| !w.is_empty()) {
                params.set(YES_LABEL, yes.to_string());
            }
            if let Some(no) = words.next().filter(|w| !w.is_empty()) {
                params.set(NO_LABEL, no.to_string());
            }
            // `only_yes` (bit 8) and `focus_yes` (bit 9).
            if flags & (1 << 8) != 0 {
                params.set(ONLY_YES, "1");
            }
            if flags & (1 << 9) != 0 {
                params.set(FOCUS_YES, "1");
            }
            Some((params, Vec::new()))
        }
        _ => None,
    }
}

/// A host dialog: the inner screen and the request it answers.
pub(crate) struct HostDialog {
    inner: alloc::boxed::Box<dyn Screen>,
    store: Shared<UiStore>,
    /// The instance the host opened (the only one that may answer).
    instance: u32,
    request: u32,
    kind: u32,
    options: Vec<String>,
}

impl HostDialog {
    pub(super) fn new(
        inner: alloc::boxed::Box<dyn Screen>,
        store: Shared<UiStore>,
        instance: u32,
        request: u32,
        kind: u32,
        options: Vec<String>,
    ) -> Self {
        Self {
            inner,
            store,
            instance,
            request,
            kind,
            options,
        }
    }

    /// Runs `handle` on the inner screen with a private output, answers the
    /// request from the reply it made, and hands the rest of its output (the
    /// *Back* that closes it, redraws, refreshes) to `cx`.
    fn through(
        &mut self,
        cx: &mut Cx<'_, '_>,
        handle: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        let mut own = Output::default();
        let handled = {
            let mut inner = Cx::new(cx.env(), &mut own).with_params(cx.params());
            handle(self.inner.as_mut(), &mut inner)
        };
        for command in own.commands.drain(..) {
            match command {
                Command::Emit(event, reply) if event.as_str() == REPLY => self.answer(&reply),
                other => cx.command(other),
            }
        }
        if let Some(mode) = own.refresh_set {
            cx.set_refresh(mode);
        }
        if let Some(mode) = own.refresh {
            cx.request_refresh(mode);
        }
        if own.redraw {
            cx.request_redraw();
        }
        if let Some(area) = own.redraw_area {
            cx.request_redraw_area(area);
        }
        if let Some(at) = own.wake_at {
            cx.wake_at(at);
        }
        handled
    }

    /// The final answer the inner screen's `reply` says, to the store; one the
    /// list does not hold is a cancel.
    fn answer(&self, reply: &Params) {
        let (outcome, choice) = match self.kind {
            2 => match reply.get(ANSWER) {
                Some("yes") => (0, 1),
                Some("no") => (0, 0),
                _ => (1, 0),
            },
            _ => reply
                .get(OPTION)
                .and_then(|option| self.options.iter().position(|o| o == option))
                .and_then(|index| u32::try_from(index).ok())
                .map_or((1, 0), |index| (0, index)),
        };
        // Refused (a first answer won already, or the request is gone): the
        // dialog closes all the same.
        let _ = self
            .store
            .answer(self.instance, self.request, outcome, choice);
    }
}

impl Screen for HostDialog {
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        self.inner.region(env)
    }

    fn dims_below(&self) -> bool {
        self.inner.dims_below()
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        self.inner.hint(action, env)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.inner.draw(canvas, env);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        self.through(cx, |inner, inner_cx| inner.on_action(action, inner_cx))
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        self.through(cx, |inner, inner_cx| inner.on_gesture(gesture, inner_cx))
    }
}
