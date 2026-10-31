//! Wikipedia: a word looked up online, the short summary of its article,
//! in the language of the book (else of PaperPaw).
//!
//! A lookup provider that goes online (see `paperpaw_sdk::lookup`): the
//! dictionary's panel asks it once *Online* is chosen and shows the
//! summary as an entry beside the dictionaries', from which the reader
//! opens it on a screen of its own. Other online sources (Wiktionary,
//! a translator) join the same way, as plugins.
//!
//! That screen shows the summary given to it (`text`), else fetches it;
//! it is bound to the slot `core.lookup-online` too, for lookup screens
//! without providers, and another plugin can take its place. The
//! summary shows over the page like a dictionary entry: the page buttons
//! or taps scroll it, confirm or a tap at its end closes it.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::json;
use paperpaw_sdk::lookup::{LookupAnswer, LookupEntry, LookupProviderSpec, LookupQuery};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::web::encode;
use paperpaw_sdk::{
    Canvas, Color, Command, Commands, Cx, Env, FixedScreen, Gesture, GestureKind, Handled,
    HttpRequest, Id, Manifest, Params, Plugin, Priority, Rect, RefreshMode, RegisterError,
    Registrar, Screen,
};

pub const PLUGIN: Id = Id::new_static("wikipedia");
/// The summary on a screen of its own; parameters `word` and `language`.
pub const SCREEN: Id = Id::new_static("wikipedia.lookup");
/// Wikipedia as a lookup provider.
pub const PROVIDER: Id = Id::new_static("wikipedia.summary");
/// The answer to the screen's summary request.
const ANSWERED: Event<HttpReply> = Event::new("wikipedia.answered");
/// The answer to the provider's summary request, with the query in its
/// parameters (`word`, `language` and [`REPLY`]).
const PROVIDED: Event<HttpReply> = Event::new("wikipedia.provided");
/// The event the query is answered with.
const REPLY: &str = "reply";
/// The longest summary read (they are a paragraph or two).
const MAX_BODY: usize = 16 * 1024;
/// How long the provider's request may take: less than the dictionary's
/// panel waits for answers (20 s), so that an answer is never thrown away
/// after the radio did the work.
const PROVIDER_TIMEOUT_S: u64 = 15;

/// The wiki's language for `language` (`en-GB` → `en`).
fn wiki(language: &str) -> String {
    let code: String = language
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_lowercase();
    if code.is_empty() {
        String::from("en")
    } else {
        code
    }
}

/// The address of the summary of `word`'s article.
fn summary_url(language: &str, word: &str) -> String {
    let title = encode(word.trim()).replace('+', "_");
    format!(
        "https://{}.wikipedia.org/api/rest_v1/page/summary/{title}",
        wiki(language)
    )
}

/// The address of the article `title`, as its source is named (without
/// `https://`, short enough to read).
fn article_url(language: &str, title: &str) -> String {
    let title = encode(title.trim()).replace('+', "_");
    format!("{}.wikipedia.org/wiki/{title}", wiki(language))
}

/// The request for the summary of `word`'s article, answered with
/// `reply`.
fn summary_request(language: &str, word: &str, reply: Id) -> HttpRequest {
    HttpRequest::get(&summary_url(language, word), reply)
        .header("User-Agent", "PaperPaw/0.1 (e-reader)")
        .header("Accept", "application/json")
        .max_body(MAX_BODY)
}

/// The summary in `reply`, with the article's title; else why there is
/// none, to show.
fn summary(env: &Env<'_>, reply: &HttpReply) -> Result<(Option<String>, String), String> {
    let why = |key: &str| env.i18n().tr(key).to_string();
    match reply.status {
        200..=299 => {
            let text = reply.text();
            let members = json::object(&text);
            match json::text(&members, "extract").filter(|t| !t.trim().is_empty()) {
                Some(extract) => Ok((
                    json::text(&members, "title").map(|t| t.to_string()),
                    extract.to_string(),
                )),
                None => Err(why("wikipedia.not-found")),
            }
        }
        404 => Err(why("wikipedia.not-found")),
        0 => Err(why("wikipedia.unreachable")),
        _ => Err(why("wikipedia.failed")),
    }
}

/// Asks for the summary of the query's word, to answer it later.
fn provide(env: &Env<'_>, query: &LookupQuery) -> LookupAnswer {
    let language = query
        .language
        .clone()
        .unwrap_or_else(|| env.i18n().language().to_string());
    let context = Params::new()
        .with(params::WORD, query.word.clone())
        .with(params::LANGUAGE, language.clone())
        .with(REPLY, query.reply.as_str().to_string());
    let request = summary_request(&language, &query.word, PROVIDED.id().clone())
        .with(context)
        .timeout(core::time::Duration::from_secs(PROVIDER_TIMEOUT_S));
    LookupAnswer::Pending(alloc::vec![Command::http(request)])
}

/// The summary arrived: the query answered with it, the whole screen
/// for more; or with why there is none.
fn provided(env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
    let context = &reply.context;
    let (Some(word), Some(to)) = (
        context.get(params::WORD),
        context.get(REPLY).and_then(|r| Id::parse(r).ok()),
    ) else {
        return;
    };
    let language = context.get(params::LANGUAGE).map(ToString::to_string);
    let query = LookupQuery::new(word, language).reply(to);
    let source = env.i18n().tr("wikipedia.title");
    let entry = match summary(env, reply) {
        Ok((_, text)) => LookupEntry::new(source, text).more(SCREEN),
        Err(why) => LookupEntry::new(source, why),
    };
    out.push(query.answer_entry(&PROVIDER, entry));
}

/// The Wikipedia plugin.
#[derive(Debug, Default)]
pub struct Wikipedia;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(
    SCREEN,
    "wikipedia.title",
    |env, params| Box::new(Summary::new(env, params)),
)];

impl Plugin for Wikipedia {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Wikipedia lookup", paperpaw_sdk::package_version!())
            .description("Looks words up online, beside the dictionaries.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.bind_slot(slots::LOOKUP_ONLINE, SCREEN, Priority::DEFAULT)?;
        r.add_lookup_provider(
            LookupProviderSpec::new(PROVIDER, "wikipedia.title", provide).online(),
        )?;
        r.on(PROVIDED, provided)?;
        // The articles' licence asks for their source and licence to be
        // named: below each, and here.
        r.contribute(
            Id::new_static("about.lines"),
            "Article summaries from Wikipedia (https://www.wikipedia.org), under CC BY-SA 4.0.",
        )?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

struct Summary {
    style: UiStyle,
    region: Rect,
    word: String,
    language: String,
    /// The summary or a message, and its lines as byte ranges.
    text: String,
    lines: Vec<(usize, usize)>,
    top: usize,
    asked: bool,
}

impl Summary {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let top = display.height * 9 / 20;
        let language = params
            .get(params::LANGUAGE)
            .map_or_else(|| env.i18n().language().to_string(), ToString::to_string);
        let mut summary = Self {
            style,
            region: Rect::new(0, top, display.width, display.height - top),
            word: params.get(params::WORD).unwrap_or_default().to_string(),
            language,
            text: String::new(),
            lines: Vec::new(),
            top: 0,
            asked: false,
        };
        // The summary given (by the dictionary's panel, which has it):
        // not fetched again.
        match params.get(params::TEXT).filter(|t| !t.trim().is_empty()) {
            Some(text) => {
                summary.asked = true;
                summary.set_text(env, text.to_string());
            }
            None => {
                let loading = env.i18n().tr("wikipedia.loading").to_string();
                summary.set_text(env, loading);
            }
        }
        summary
    }

    fn set_text(&mut self, env: &Env<'_>, text: String) {
        self.lines.clear();
        self.top = 0;
        if let Some(font) = self.style.font(env) {
            let width = f32::from(self.region.width) - 2.0 * self.style.margin() as f32;
            let mut start = 0;
            for paragraph in text.split('\n') {
                for range in font.wrap(self.style.text(), paragraph, width) {
                    self.lines.push((start + range.start, start + range.end));
                }
                start += paragraph.len() + 1;
            }
        }
        self.text = text;
    }

    fn header(&self) -> Rect {
        Rect::new(
            self.region.x,
            self.region.y,
            self.region.width,
            self.style.row(),
        )
    }

    fn line_height(&self, env: &Env<'_>) -> f32 {
        self.style.font(env).map_or(self.style.text() * 1.3, |f| {
            f.line_metrics(self.style.text()).line_height
        })
    }

    fn rows(&self, env: &Env<'_>) -> usize {
        let height = f32::from(self.region.height)
            - f32::from(self.header().height)
            - self.style.gap() as f32;
        ((height / self.line_height(env)) as usize).max(1)
    }

    fn scroll(&mut self, env: &Env<'_>, forward: bool, cx: &mut Cx<'_, '_>) -> bool {
        let rows = self.rows(env);
        let top = if forward {
            let next = self.top + rows;
            if next >= self.lines.len() {
                return false;
            }
            next
        } else {
            if self.top == 0 {
                return false;
            }
            self.top.saturating_sub(rows)
        };
        self.top = top;
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
        true
    }

    /// The summary arrived: its title and text, with its source and
    /// licence below it (CC BY-SA asks for both), or why not.
    fn answered(&mut self, env: &Env<'_>, reply: &HttpReply) {
        let text = match summary(env, reply) {
            Ok((title, text)) => {
                if let Some(title) = title {
                    self.word = title;
                }
                let source = article_url(&self.language, &self.word);
                let from = env.i18n().format(
                    "wikipedia.source",
                    &[("address", paperpaw_sdk::Arg::Str(&source))],
                );
                format!("{text}\n\n{from}")
            }
            Err(why) => why,
        };
        self.set_text(env, text);
    }
}

impl Screen for Summary {
    /// *Select* closes it; the arrows scroll.
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM {
            paperpaw_sdk::Hint::Text(env.i18n().tr("core.hint.close").to_string())
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let r = self.region;
        ui.canvas().fill(r, Color::WHITE);
        ui.canvas().fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            (u32::from(style.line()) * 2).max(2),
            Color::BLACK,
        );
        let Some(font) = ui.font() else {
            return;
        };
        let header = self.header();
        let margin = style.margin() as f32;
        let source = env.i18n().tr("wikipedia.title");
        let source_width = font.measure(style.small(), source);
        let right = f32::from(header.width) - margin - source_width;
        ui::Label::new(source)
            .px(style.small())
            .color(style.faint(ui::Ink::DETAIL))
            .draw_from(&mut ui, right, header);
        let word = ui::fit(&font, style.title(), &self.word, right - margin * 2.0);
        ui::Label::new(&word)
            .px(style.title())
            .color(Color::BLACK)
            .draw_from(&mut ui, margin, header);
        let height = self.line_height(env);
        let ascent = font.line_metrics(style.text()).ascent;
        let top = f32::from(header.y + header.height) + style.gap() as f32 / 2.0;
        let rows = self.rows(env);
        for (n, &(start, end)) in self.lines.iter().skip(self.top).take(rows).enumerate() {
            let baseline = top + height * n as f32 + ascent;
            font.draw(
                ui.canvas(),
                style.text(),
                margin,
                baseline as i32,
                &self.text[start..end],
                Color::BLACK,
            );
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.asked {
            return;
        }
        self.asked = true;
        let env = *cx.env();
        if !env.caps().network {
            let text = env.i18n().tr("wikipedia.no-network").to_string();
            self.set_text(&env, text);
            cx.request_redraw();
            return;
        }
        let request = summary_request(&self.language, &self.word, ANSWERED.id().clone());
        cx.command(Command::http(request));
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = ANSWERED.read(event, params) {
            let env = *cx.env();
            self.answered(&env, &reply);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let a = action;
        if [
            actions::NEXT_PAGE,
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
        ]
        .contains(a)
        {
            self.scroll(&env, true, cx);
        } else if [
            actions::PREV_PAGE,
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
        ]
        .contains(a)
        {
            self.scroll(&env, false, cx);
        } else if *a == actions::CONFIRM {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap || !self.region.contains(gesture.start) {
            return Handled::No;
        }
        let env = *cx.env();
        let forward = gesture.start.x > i32::from(self.region.width) / 3;
        if !self.scroll(&env, forward, cx) && forward {
            cx.command(Command::Back);
        }
        Handled::Yes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn addresses() {
        assert_eq!(
            summary_url("de-DE", "Hund"),
            "https://de.wikipedia.org/api/rest_v1/page/summary/Hund"
        );
        assert_eq!(
            summary_url("en", "white rabbit"),
            "https://en.wikipedia.org/api/rest_v1/page/summary/white_rabbit"
        );
        assert_eq!(wiki(""), "en");
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}

#[cfg(test)]
mod address_tests;
