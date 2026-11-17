//! The host's own screens (#293, Decision 209): those no plugin may or can
//! draw. The firmware confirmation (`docs/abi/device.md` 5.0: the host's own
//! words and screen), the placeholder under a slot nobody serves (all a
//! safe-mode start shows), the screen that finishes an update before the
//! plugins start, the notice where no plugin shows notices, and what holds a
//! screen's place while it is made anew. Drawn by the small painter in
//! [`paint`] from the kernel's fonts, settings and texts.
mod confirm;
mod finishing;
mod notice;
mod paint;
mod placeholder;

use alloc::boxed::Box;

use paperpaw_sdk::builtin::screens;
use paperpaw_sdk::{
    Canvas, CloseCause, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Params, PointerEvent,
    Rect, TitleEntry,
};

pub(crate) use notice::Notice;

/// The notice's screen, bound to the notice slot as a fallback.
pub(crate) const NOTICE: Id = notice::SCREEN;

/// Which of the host's screens an entry makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Placeholder,
    Finishing,
    Notice,
    Confirm,
}

/// A host screen as the registry lists it: its id, the translation key of
/// its name and what it is.
#[derive(Debug)]
pub(crate) struct HostEntry {
    entry: TitleEntry,
    kind: Kind,
}

impl HostEntry {
    pub(crate) const fn new(id: Id, title: &'static str, kind: Kind) -> Self {
        Self {
            entry: TitleEntry::new(id, title),
            kind,
        }
    }

    pub(crate) const fn id(&self) -> &Id {
        &self.entry.id
    }

    /// Its id and the translation key of its name.
    pub(crate) const fn entry(&self) -> &TitleEntry {
        &self.entry
    }

    /// The screen, opened with `params`.
    pub(crate) fn make(&self, env: &Env<'_>, params: &Params) -> HostScreen {
        match self.kind {
            Kind::Placeholder => HostScreen::Placeholder,
            Kind::Finishing => HostScreen::Finishing(Box::new(finishing::Finishing::new(params))),
            Kind::Notice => HostScreen::Notice(Box::new(Notice::new(env, params))),
            Kind::Confirm => HostScreen::Confirm(Box::new(confirm::Confirm::new(env, params))),
        }
    }
}

/// The host's screens a kernel lists, by id.
pub(crate) static ENTRIES: [HostEntry; 4] = [
    HostEntry::new(notice::SCREEN, notice::TITLE, Kind::Notice),
    HostEntry::new(screens::PLACEHOLDER, placeholder::TEXT, Kind::Placeholder),
    HostEntry::new(
        screens::FINISHING_UPDATE,
        "core.finishing-update.title",
        Kind::Finishing,
    ),
    HostEntry::new(
        crate::native_host::firmware_confirm::SCREEN,
        "core.firmware.title",
        Kind::Confirm,
    ),
];

/// One of the host's screens on the stack.
pub(crate) enum HostScreen {
    Placeholder,
    /// What a screen's place holds between the old screen dropped and the
    /// new one made (`Kernel::remake_screen`): never drawn nor asked anything.
    Gone,
    Finishing(Box<finishing::Finishing>),
    Notice(Box<Notice>),
    Confirm(Box<confirm::Confirm>),
}

impl HostScreen {
    pub(crate) fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        match self {
            Self::Placeholder => placeholder::draw(canvas, env),
            Self::Finishing(screen) => screen.draw(canvas, env),
            Self::Notice(screen) => screen.draw(canvas, env),
            Self::Confirm(screen) => screen.draw(canvas, env),
            Self::Gone => {}
        }
    }

    pub(crate) fn region(&self) -> Option<Rect> {
        match self {
            Self::Notice(screen) => Some(screen.region()),
            Self::Confirm(screen) => Some(screen.region()),
            _ => None,
        }
    }

    pub(crate) fn dims_below(&self) -> bool {
        matches!(self, Self::Confirm(_))
    }

    /// Nothing shows above it and no plugin's input reaches it: the
    /// confirmation, and an update finishing while it holds on.
    pub(crate) fn guards(&self) -> bool {
        match self {
            Self::Confirm(_) => true,
            Self::Finishing(screen) => screen.holding(),
            _ => false,
        }
    }

    /// It holds the edge swipes while it guards.
    pub(crate) fn holds_edges(&self) -> bool {
        self.guards()
    }

    pub(crate) fn hints(&self) -> Option<HintVisibility> {
        match self {
            Self::Confirm(_) => Some(HintVisibility::Hidden),
            Self::Finishing(screen) => screen.hints(),
            _ => None,
        }
    }

    pub(crate) fn hint(&self, action: &Id) -> Hint {
        match self {
            Self::Confirm(screen) => screen.hint(action),
            Self::Finishing(screen) => screen.hint(action),
            _ => Hint::Default,
        }
    }

    pub(crate) fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        match self {
            Self::Confirm(screen) => screen.on_show(cx),
            Self::Finishing(screen) => screen.on_show(cx),
            _ => {}
        }
    }

    pub(crate) fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        match self {
            Self::Confirm(screen) => screen.on_timer(cx),
            Self::Finishing(screen) => screen.on_timer(cx),
            _ => {}
        }
    }

    pub(crate) fn on_close_request(&mut self, _: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Confirm(screen) => screen.on_close_request(cx),
            Self::Finishing(screen) => screen.taken(),
            _ => Handled::No,
        }
    }

    pub(crate) fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Confirm(screen) => screen.on_action(action, cx),
            Self::Finishing(screen) => screen.taken(),
            _ => Handled::No,
        }
    }

    pub(crate) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Confirm(screen) => screen.on_gesture(gesture, cx),
            Self::Finishing(screen) => screen.taken(),
            _ => Handled::No,
        }
    }

    pub(crate) fn on_pointer(&mut self, _: &PointerEvent) -> Handled {
        match self {
            Self::Finishing(screen) => screen.taken(),
            _ => Handled::No,
        }
    }
}
