//! The one-time low-memory notice (#248, Senior's recommendation, no hard
//! limit for Beta 4): where a shell finds the memory short with store plugins
//! loaded (the X4 shell's rule, `paperpaw_x4::low_memory`), it asks the kernel
//! to say so once. The notice is a kernel-owned dialog on the host's
//! `ConfirmBox`: its words, *Plugins* (opens the plugin manager's list) and
//! *Not now* (or *Back*). The kernel shows it only over the home screen,
//! never over another screen (the first-run setup, a book, a dialog, the
//! sleep screen), and nothing of it is kept once it closes.
use super::dialogs::{ConfirmBox, Touch};
use crate::{Effects, Kernel};
use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Params, Rect, Screen,
};

/// The screen's id: a host screen, opened only by the kernel.
pub(crate) const SCREEN: Id = Id::new_static("core.low-memory");
/// The plugin manager's list, where *Plugins* goes.
const PLUGINS: Id = Id::new_static("plugin-manager.list");
const TITLE: &str = "title";
const TEXT: &str = "text";
const PLUGINS_WORD: &str = "plugins";
const LATER_WORD: &str = "later";

/// The notice's screen.
pub(crate) struct LowMemory {
    board: ConfirmBox,
    labels: [alloc::string::String; 2],
}

impl LowMemory {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let get = |key| alloc::string::String::from(params.get(key).unwrap_or(""));
        let labels = [get(PLUGINS_WORD), get(LATER_WORD)];
        let board = ConfirmBox::new(
            env,
            &get(TITLE),
            &get(TEXT),
            &[labels[0].as_str(), labels[1].as_str()],
            0,
        );
        Self { board, labels }
    }

    /// Closes; *Plugins* (the first button) goes to the plugin manager.
    fn answer(&self, button: usize, cx: &mut Cx<'_, '_>) {
        cx.close();
        if button == 0 {
            cx.command(Command::OpenScreen(PLUGINS, Params::new()));
        }
    }
}

impl Screen for LowMemory {
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        Some(self.board.region(env))
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn hints(&self) -> Option<HintVisibility> {
        Some(HintVisibility::Hidden)
    }

    fn hint(&self, action: &Id, _: &Env<'_>) -> Hint {
        if *action == actions::CONFIRM {
            Hint::Text(self.labels[self.board.focus().min(1)].clone())
        } else {
            Hint::Hidden
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.board.draw(canvas, env);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            self.answer(self.board.focus(), cx);
        } else if *action == actions::BACK {
            self.answer(1, cx);
        } else {
            let _ = self.board.on_action(action, cx);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if let Touch::Button(at) = self.board.on_gesture(gesture, cx) {
            self.answer(at, cx);
        }
        Handled::Yes
    }
}

impl Kernel {
    /// Whether the home screen is on top, with nothing over it.
    pub fn home_on_top(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|top| top.slot.as_ref() == Some(&slots::HOME) && top.id != SCREEN)
    }

    /// Shows the low-memory notice, once the caller has decided it is due
    /// (the shell's rule); `true` where it shows. Only over the home screen
    /// with nothing above it and nothing guarding the device: anywhere else
    /// (or where the screen could not open) it does nothing and the caller
    /// asks again later.
    pub fn show_low_memory_notice(&mut self, now: Instant) -> (bool, Effects) {
        if !self.home_on_top() || self.held_by_guard() || self.notice.is_some() {
            return (false, Effects::default());
        }
        self.now = now;
        let tr = &self.shared.translator;
        let params = Params::new()
            .with(TITLE, tr.tr("core.low-memory.title"))
            .with(TEXT, tr.tr("core.low-memory.text"))
            .with(PLUGINS_WORD, tr.tr("core.low-memory.plugins"))
            .with(LATER_WORD, tr.tr("core.low-memory.later"));
        self.open(SCREEN, None, &params);
        let shown = self.stack.last().is_some_and(|top| top.id == SCREEN);
        (shown, self.finish())
    }
}
