//! A sample interlinear reading: a small translation above each line of
//! a book, and as a second way a translation after each paragraph (a
//! parallel text). It shows what such a plugin takes from PaperPaw:
//!
//! - a **text filter** that asks for room above every line of a
//!   paragraph (`Block::leading_above`), so the layout sets the lines
//!   apart and pages break with it, and that inserts a translated
//!   paragraph after each one (`Block::insert_after`), which shows but
//!   takes no place in the book;
//! - a **page layer** that asks for the page's words (`words()`), draws
//!   above each line the translation of each sentence part on it,
//!   starting where the part's first word starts, and says where its
//!   items are (`items()`): the room above each line, which a tap (or
//!   Select, chosen with the buttons as links are) opens to the whole
//!   translation and closes again.
//!
//! The translation is a toy: a few English words in German from a small
//! word list, the others as they are. No network, no files.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, groups};
use paperpaw_sdk::document::{Block, BlockKind, Locator, Run, TextStyle};
use paperpaw_sdk::filters::TextFilterSpec;
use paperpaw_sdk::layers::{LayerItem, LineBox, PageLayerSpec, PageView};
use paperpaw_sdk::reading::WordBox;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Env, Id, Manifest, Plugin, Rect, RefreshMode, RegisterError, Registrar,
    SettingSpec, Version,
};

/// Ids start with the plugin's id: `interlinear.…`.
pub const PLUGIN: Id = Id::new_static("interlinear");
/// A translation above each line.
pub const ABOVE: Setting<bool> = Setting::new("interlinear.above");
/// A translation after each paragraph.
pub const PARALLEL: Setting<bool> = Setting::new("interlinear.parallel");
const LAYER: Id = Id::new_static("interlinear.layer");
const FILTER: Id = Id::new_static("interlinear.filter");

/// Room above each line: three quarters of the text's size.
const LEADING: u16 = 75;

static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::switch(
        ABOVE.static_str(),
        "interlinear.above",
        true,
        groups::READING,
    ),
    SettingSpec::switch(
        PARALLEL.static_str(),
        "interlinear.parallel",
        false,
        groups::READING,
    ),
];

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// The toy word list: English to German, lower case.
const WORDS: &[(&str, &str)] = &[
    ("a", "ein"),
    ("all", "alle"),
    ("and", "und"),
    ("as", "wie"),
    ("at", "an"),
    ("be", "sein"),
    ("but", "aber"),
    ("by", "von"),
    ("for", "für"),
    ("had", "hatte"),
    ("her", "ihr"),
    ("herself", "sich"),
    ("in", "in"),
    ("it", "es"),
    ("little", "klein"),
    ("not", "nicht"),
    ("of", "von"),
    ("on", "auf"),
    ("or", "oder"),
    ("rabbit", "Kaninchen"),
    ("said", "sagte"),
    ("she", "sie"),
    ("so", "so"),
    ("that", "dass"),
    ("the", "die"),
    ("there", "dort"),
    ("thought", "dachte"),
    ("time", "Zeit"),
    ("to", "zu"),
    ("very", "sehr"),
    ("was", "war"),
    ("way", "Weg"),
    ("what", "was"),
    ("when", "als"),
    ("with", "mit"),
    ("you", "du"),
];

/// `word` in the toy translation: from the list where it is there, its
/// punctuation kept; else as it is.
pub fn translate_word(word: &str) -> String {
    let start = word
        .find(|c: char| c.is_alphanumeric())
        .unwrap_or(word.len());
    let end = word
        .rfind(|c: char| c.is_alphanumeric())
        .map_or(start, |i| {
            i + word[i..].chars().next().map_or(1, char::len_utf8)
        });
    let core = &word[start..end];
    let lower = core.to_lowercase();
    match WORDS.binary_search_by(|(en, _)| (*en).cmp(lower.as_str())) {
        Ok(at) => format!("{}{}{}", &word[..start], WORDS[at].1, &word[end..]),
        Err(_) => String::from(word),
    }
}

/// `text` in the toy translation, word by word.
pub fn translate(text: &str) -> String {
    text.split(' ')
        .map(translate_word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `word` ends a sentence.
fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '’', '”', ')'])
        .ends_with(['.', '!', '?'])
}

/// The lines whose translation is open, by where they start; shared by
/// the layer and the handler of its taps.
type Open = Rc<RefCell<Vec<Locator>>>;

/// The plugin.
#[derive(Debug, Default)]
pub struct Interlinear;

impl Plugin for Interlinear {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Interlinear", Version::new(0, 1, 0))
            .description("A translation above each line, or after each paragraph.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_text_filter(TextFilterSpec::new(
            FILTER,
            "interlinear.title",
            Box::new(filter),
            Box::new(|env: &Env<'_>| {
                let s = env.settings();
                u64::from(s.read(&ABOVE)) | (u64::from(s.read(&PARALLEL)) << 1)
            }),
        ))?;
        let open = Open::default();
        let shown = open.clone();
        r.add_page_layer(
            PageLayerSpec::new(LAYER, "interlinear.title", move |canvas, env, page| {
                draw(canvas, env, page, &shown.borrow());
            })
            .switch(&ABOVE)
            .words()
            .items(|_, page| items(page)),
        )?;
        r.on(events::PAGE_LAYER_TAPPED, move |_, tapped, out| {
            if tapped.layer != LAYER || tapped.item.is_none() {
                return;
            }
            let Some(line) = tapped.line() else {
                return;
            };
            let mut open = open.borrow_mut();
            match open.iter().position(|l| *l == line) {
                Some(at) => {
                    open.remove(at);
                }
                None => open.push(line),
            }
            out.push(Command::Redraw(RefreshMode::Fast));
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Room above each paragraph's lines, and a translation after it.
fn filter(env: &Env<'_>, block: &mut Block) {
    if block.kind != BlockKind::Paragraph || block.text.is_empty() {
        return;
    }
    let s = env.settings();
    if s.read(&ABOVE) {
        block.leading_above = Some(LEADING);
    }
    if s.read(&PARALLEL) {
        let text = translate(&block.text);
        let len = text.len() as u32;
        let run = Run::new(0, len, TextStyle::ITALIC);
        // Past its bounds (a very long paragraph) it is left out.
        let _ = block
            .insert_after(Block::new(BlockKind::Paragraph, 0, len, text).runs(alloc::vec![run]));
    }
}

/// The room above each line with room above it: tapped, it opens or
/// closes the line's translation.
fn items(page: &PageView<'_>) -> Vec<LayerItem> {
    page.lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.leading > 0)
        .map(|(i, line)| LayerItem::new(i as u32, room(line)).label("Translation"))
        .collect()
}

/// The room above `line`.
fn room(line: &LineBox) -> Rect {
    let a = line.area;
    Rect::new(a.x, a.y.saturating_sub(line.leading), a.width, line.leading)
}

/// Above each line, the translation of each sentence part on it from
/// where its first word starts: its first word alone and "…" while the
/// line is closed, all of it while open.
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, page: &PageView<'_>, open: &[Locator]) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    for line in page.lines.iter().filter(|l| l.leading > 0) {
        let words: Vec<&WordBox> = page
            .words
            .iter()
            .filter(|w| w.start >= line.start && w.start < line.end)
            .collect();
        let Some(first) = words.first() else {
            continue;
        };
        let size = (f32::from(line.leading) * 0.7).max(8.0);
        let baseline = i32::from(line.area.y) - i32::from(line.leading) / 4;
        let is_open = open.contains(&line.start);
        // Where each sentence part on the line starts.
        let mut parts: Vec<(i32, Vec<&str>)> = Vec::new();
        let before_ends = page
            .words
            .iter()
            .take_while(|w| w.start < first.start)
            .last()
            .is_none_or(|w| ends_sentence(&w.text));
        let mut starts_part = before_ends;
        for word in &words {
            if starts_part || parts.is_empty() {
                parts.push((i32::from(word.area.x), Vec::new()));
            }
            if let Some((_, part)) = parts.last_mut() {
                part.push(word.text.as_str());
            }
            starts_part = ends_sentence(&word.text);
        }
        let right = line.area.right() as i32;
        for (n, (x, part)) in parts.iter().enumerate() {
            let next = parts.get(n + 1).map_or(right, |(x, _)| *x);
            let text = match is_open {
                true => translate(&part.join(" ")),
                false => format!("{} …", translate_word(part[0])),
            };
            let room = (next - x - 4).max(0) as f32;
            let shown = ui::fit(&font, size, &text, room);
            font.draw(canvas, size, *x as f32, baseline, &shown, Color::gray(80));
        }
    }
}
