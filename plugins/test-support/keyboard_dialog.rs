//! Test-only: a stand-in for the keyboard dialog, for tests of plugins that
//! ask for text on a kernel that registers interface-6 plugins. The real
//! keyboard is a native plugin now (`plugins/keyboard`); the host does not run
//! it yet, so these tests could not open it. The stand-in fills the slot
//! `core.keyboard` with a plain screen of the question's title and one key
//! each for the letters and digits, a switch between the two, and *OK*: *OK*
//! answers with the text (the reply event of the question, as the keyboard
//! does) and Back cancels. It is never part of a build that ships.
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Instances, Manifest,
    Params, Plugin, Priority, Rect, RegisterError, Registrar, Screen, ScreenSpec, Version,
};

/// The screen the slot `core.keyboard` shows, as the keyboard's.
pub const SCREEN: Id = Id::new_static("keyboard.screen");

/// Keys a row holds.
const PER_ROW: usize = 7;

/// The stand-in plugin.
pub struct KeyboardDialog;

impl Plugin for KeyboardDialog {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("keyboard"),
            "Keyboard",
            Version::new(0, 1, 0),
        )
        .description("A stand-in for the keyboard dialog, in tests.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(
            ScreenSpec::new(
                SCREEN,
                "Keyboard",
                Box::new(|env, params| Box::new(Dialog::new(env, params))),
            )
            .instances(Instances::MANY),
        )?;
        r.bind_slot(slots::KEYBOARD, SCREEN, Priority::DEFAULT)
    }
}

struct Dialog {
    params: Params,
    title: String,
    text: String,
    /// Whether the digits are the keys.
    digits: bool,
    /// The key with the focus (buttons).
    focus: usize,
    area: Rect,
}

impl Dialog {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        Self {
            params: params.clone(),
            title: params.get(params::TITLE).unwrap_or_default().to_string(),
            text: params.get(params::TEXT).unwrap_or_default().to_string(),
            digits: false,
            focus: 0,
            area: env.content(),
        }
    }

    /// The labels of the keys now.
    fn labels(&self) -> Vec<String> {
        let mut keys: Vec<String> = if self.digits {
            ('0'..='9').map(String::from).collect()
        } else {
            ('a'..='z').map(String::from).collect()
        };
        keys.push(if self.digits { "ABC" } else { "123" }.to_string());
        keys.push("OK".to_string());
        keys
    }

    /// Where key `index` of `count` lies: rows of [`PER_ROW`] in the lower
    /// half of the screen.
    fn key_area(&self, index: usize, count: usize) -> Rect {
        let rows = count.div_ceil(PER_ROW).max(1) as u16;
        let (width, height) = (
            self.area.width / PER_ROW as u16,
            self.area.height / 2 / rows,
        );
        let (row, column) = ((index / PER_ROW) as u16, (index % PER_ROW) as u16);
        Rect::new(
            self.area.x + column * width,
            self.area.y + self.area.height / 2 + row * height,
            width,
            height,
        )
    }

    fn press(&mut self, label: &str, cx: &mut Cx<'_, '_>) {
        match label {
            "123" | "ABC" => {
                self.digits = !self.digits;
                self.focus = 0;
            }
            "OK" => {
                cx.command(Command::Back);
                let text = core::mem::take(&mut self.text);
                if let Some(reply) = self
                    .params
                    .get(params::REPLY)
                    .and_then(|r| Id::parse(r).ok())
                {
                    let typed = Typed::new(text, self.params.clone());
                    cx.command(Command::Emit(reply, typed.to_params()));
                }
            }
            letter => self.text.push_str(letter),
        }
        cx.request_redraw();
    }
}

impl Screen for Dialog {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill(self.area, Color::WHITE);
        let top = Rect::new(
            self.area.x,
            self.area.y,
            self.area.width,
            self.area.height / 8,
        );
        canvas.describe(Role::Heading, &self.title, top);
        let field = Rect::new(top.x, top.y + top.height, top.width, top.height);
        canvas.describe(Role::Field, &self.text, field);
        let labels = self.labels();
        for (index, label) in labels.iter().enumerate() {
            let area = self.key_area(index, labels.len());
            canvas
                .describe(Role::Button, label, area)
                .focused(index == self.focus);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let count = self.labels().len();
        if *action == actions::BACK {
            cx.command(Command::Back);
        } else if *action == actions::NEXT || *action == actions::RIGHT {
            self.focus = (self.focus + 1) % count;
            cx.request_redraw();
        } else if *action == actions::PREVIOUS || *action == actions::LEFT {
            self.focus = (self.focus + count - 1) % count;
            cx.request_redraw();
        } else if *action == actions::CONFIRM {
            let label = self.labels().swap_remove(self.focus.min(count - 1));
            self.press(&label, cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let labels = self.labels();
        let hit =
            (0..labels.len()).find(|&i| self.key_area(i, labels.len()).contains(gesture.start));
        if let (GestureKind::Tap, Some(index)) = (gesture.kind, hit) {
            self.press(&labels[index], cx);
        }
        Handled::Yes
    }
}
