//! *A flash card due* as the sleep screen (a mode the sleep screen offers,
//! `vocabulary.sleep-card`): the front of the card that comes next, large
//! in the middle, and below it how many are due. Waking, the review opens
//! with that card (see [`crate::wake`]), its back shown on *Select*.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Canvas, Color, Cx, Env, Handled, Id, Rect, Screen};

use crate::cards::{self, Day};
use crate::{NEW_PER_DAY, decks};

/// Where the card shown asleep is kept, for the review that opens on
/// waking: `<deck> <id>`.
pub(crate) fn shown_file() -> String {
    format!(
        "{}/cards/asleep.txt",
        paperpaw_sdk::Env::plugin_dir(crate::PLUGIN.as_str())
    )
}

/// The card shown asleep, if one was: its deck and id.
pub(crate) fn shown(env: &Env<'_>) -> Option<(u32, u32)> {
    let text = paperpaw_sdk::read_file(env.storage(), &shown_file(), 32).ok()?;
    let text = core::str::from_utf8(&text).ok()?;
    let (deck, id) = text.trim().split_once(' ')?;
    Some((deck.parse().ok()?, id.parse().ok()?))
}

pub(crate) struct SleepCard {
    /// The front of the card due next, if one is.
    front: Option<String>,
    /// "12 cards due", or that none is.
    note: String,
}

impl SleepCard {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let now = cards::now(env);
        let day = Day::now(env, now);
        let per_day = env.settings().read(&NEW_PER_DAY).clamp(0, 1000) as u32;
        let new = cards::new_left(env.storage(), day, per_day);
        let next = cards::due(env.storage(), day.end, new, 1, None)
            .into_iter()
            .next()
            .and_then(|due| Some((due, cards::card_at(env.storage(), due.deck, due.offset)?)));
        let _ = env.storage().remove(&shown_file());
        let front = next.map(|(due, card)| {
            let kept = format!("{} {}\n", due.deck, due.state.id);
            let _ = env.storage().write(&shown_file(), kept.as_bytes());
            card.front
        });
        let note = match front {
            Some(_) => {
                let count = decks::due_now(env) as i64;
                env.i18n().plural("vocabulary.sleep-card.due", count, &[])
            }
            None => env.i18n().tr("vocabulary.sleep-card.none").into(),
        };
        Self { front, note }
    }
}

impl Screen for SleepCard {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let display = env.device().display;
        let style = UiStyle::new(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        ui.canvas()
            .fill(Rect::new(0, 0, display.width, display.height), Color::WHITE);
        let Some(font) = ui.font() else {
            return;
        };
        let margin = style.margin();
        let width = f32::from(display.width.saturating_sub(2 * margin));
        let large = style.text() * 1.8;
        let big = font.line_metrics(large).line_height as u16;
        let small = font.line_metrics(style.small()).line_height as u16;
        let lines = self
            .front
            .as_deref()
            .map(|front| font.wrap(large, front, width))
            .unwrap_or_default();
        let shown = lines.len().min(6) as u16;
        let gap = style.gap() * 3;
        // The note whole, on as many lines as it takes.
        let notes = font.wrap(style.small(), &self.note, width);
        let height = shown * big + gap + small * notes.len() as u16;
        let mut y = display.height.saturating_sub(height) / 2;
        if let Some(front) = &self.front {
            for range in lines.into_iter().take(6) {
                let line = &front[range];
                let x = f32::from(margin) + (width - font.measure(large, line)).max(0.0) / 2.0;
                let row = Rect::new(margin, y, display.width.saturating_sub(2 * margin), big);
                ui::Label::new(line)
                    .px(large)
                    .color(Color::BLACK)
                    .draw_from(&mut ui, x, row);
                y += big;
            }
            y += gap;
        }
        for range in notes {
            let note = self.note[range].trim();
            let x = f32::from(margin) + (width - font.measure(style.small(), note)).max(0.0) / 2.0;
            let row = Rect::new(margin, y, display.width.saturating_sub(2 * margin), small);
            ui::Label::new(note)
                .px(style.small())
                .color(style.faint(ui::Ink::NOTE))
                .draw_from(&mut ui, x, row);
            y += small;
        }
    }

    /// Buttons are the sleep screen's (it shows this one in its place).
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}
