//! Translation: selected text translated online by DeepL, bound to the
//! slot `core.translate` (the reader offers *Translate* for selected text
//! when there is a network). Other services can bind there instead.
//!
//! DeepL needs an API key of one's own: free at deepl.com for up to
//! 500,000 characters a month. Without one, the panel says so and a tap
//! asks for it; it is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`), where it can be changed or removed. The text
//! is translated into the language chosen (setting `translate.to`), by
//! default PaperPaw's own.
//!
//! The translation shows over the page like a dictionary entry: the page
//! buttons or taps scroll it, confirm or a tap at its end closes it.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, groups, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::json;
use paperpaw_sdk::replies::{HttpReply, Typed};
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::web::encode;
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, FixedScreen, Gesture, GestureKind, Handled, HttpRequest, Id,
    Manifest, Params, Plugin, Priority, Rect, RefreshMode, RegisterError, Registrar, Screen,
    SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("translate");
/// The DeepL API key.
pub const KEY: Id = Id::new_static("translate.deepl-key");
/// The language translated into: [`AUTO`] (PaperPaw's) or one of
/// [`LANGUAGES`].
pub const TO: Setting<Text> = Setting::new("translate.to");
pub const AUTO: &str = "translate.to.auto";
const SCREEN: Id = Id::new_static("translate.panel");
const SECTION: &str = "translate.section";
/// The answer to the translation request.
const ANSWERED: Event<HttpReply> = Event::new("translate.answered");
/// The key, entered on the keyboard.
const KEY_ENTERED: Event<Typed> = Event::new("translate.key-entered");
/// Longest text sent at once, in characters.
const MAX_CHARS: usize = 3000;

/// The languages DeepL translates into, as setting values: their codes
/// follow `translate.to.`.
pub const LANGUAGES: &[&str] = &[
    "translate.to.ar",
    "translate.to.bg",
    "translate.to.cs",
    "translate.to.da",
    "translate.to.de",
    "translate.to.el",
    "translate.to.en-gb",
    "translate.to.en-us",
    "translate.to.es",
    "translate.to.et",
    "translate.to.fi",
    "translate.to.fr",
    "translate.to.hu",
    "translate.to.id",
    "translate.to.it",
    "translate.to.ja",
    "translate.to.ko",
    "translate.to.lt",
    "translate.to.lv",
    "translate.to.nb",
    "translate.to.nl",
    "translate.to.pl",
    "translate.to.pt-br",
    "translate.to.pt-pt",
    "translate.to.ro",
    "translate.to.ru",
    "translate.to.sk",
    "translate.to.sl",
    "translate.to.sv",
    "translate.to.tr",
    "translate.to.uk",
    "translate.to.zh-hans",
];

/// DeepL's target language for the setting `to`, with `ui` PaperPaw's
/// language for [`AUTO`] (`en` → `EN-GB`, `de-AT` → `DE`).
fn target(to: &str, ui: &str) -> String {
    if let Some(code) = to.strip_prefix("translate.to.")
        && to != AUTO
    {
        return code.to_uppercase();
    }
    let primary = ui.split(['-', '_']).next().unwrap_or("en").to_lowercase();
    let code = match primary.as_str() {
        "en" => "en-gb",
        "pt" => "pt-pt",
        "zh" => "zh-hans",
        "no" | "nn" => "nb",
        other => other,
    };
    if LANGUAGES
        .iter()
        .any(|l| l.strip_prefix("translate.to.") == Some(code))
    {
        code.to_uppercase()
    } else {
        String::from("EN-GB")
    }
}

/// The request translating `text` with `key` into `target`: free keys
/// (ending in `:fx`) go to DeepL's free service.
fn request(key: &str, text: &str, target: &str) -> HttpRequest {
    let host = if key.ends_with(":fx") {
        "api-free.deepl.com"
    } else {
        "api.deepl.com"
    };
    let text: String = text.chars().take(MAX_CHARS).collect();
    let body = format!("text={}&target_lang={}", encode(&text), encode(target));
    HttpRequest::new("POST", &format!("https://{host}/v2/translate"), ANSWERED)
        .header(
            String::from("Authorization"),
            format!("DeepL-Auth-Key {key}"),
        )
        .header(
            String::from("Content-Type"),
            String::from("application/x-www-form-urlencoded"),
        )
        .header(
            String::from("User-Agent"),
            String::from("PaperPaw/0.1 (e-reader)"),
        )
        .body(body)
}

/// The translation in DeepL's answer
/// (`{"translations":[{"detected_source_language":"EN","text":"…"}]}`).
fn translation(body: &str) -> Option<String> {
    let at = body.find("\"translations\"")?;
    let open = at + body[at..].find('{')?;
    let members = json::object(&body[open..]);
    json::text(&members, "text").map(ToString::to_string)
}

/// The translation plugin.
#[derive(Debug, Default)]
pub struct Translate;

/// The languages to translate into: the reader's own, then each one.
const TARGETS: [&str; LANGUAGES.len() + 1] = {
    let mut targets = [AUTO; LANGUAGES.len() + 1];
    let mut i = 0;
    while i < LANGUAGES.len() {
        targets[i + 1] = LANGUAGES[i];
        i += 1;
    }
    targets
};

/// The translation's part of the reading settings.
static IN_TRANSLATION: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().section(SECTION);

/// The language to translate into.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::choice(
    TO.static_str(),
    "translate.setting.to",
    &TARGETS,
    AUTO,
    groups::READING,
)
.advanced()
.with_extra(&IN_TRANSLATION)
.description("translate.setting.to.help")];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(
    SCREEN,
    "translate.title",
    |env, params| Box::new(Panel::new(env, params)),
)];

impl Plugin for Translate {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Translation", paperpaw_sdk::package_version!())
            .description("Translates selected text online, with DeepL.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.bind_slot(slots::TRANSLATE, SCREEN, Priority::DEFAULT)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The panel with the translation (or why there is none).
struct Panel {
    style: UiStyle,
    region: Rect,
    /// The text to translate.
    original: String,
    /// It in one line, shortened to fit left of the service's name in
    /// the header: made once, for it may be a paragraph.
    heading: String,
    /// The translation or a message, and its lines as byte ranges.
    text: String,
    lines: Vec<(usize, usize)>,
    top: usize,
    asked: bool,
    /// No key yet: a tap asks for it.
    needs_key: bool,
}

impl Panel {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let top = display.height * 9 / 20;
        let mut panel = Self {
            style,
            region: Rect::new(0, top, display.width, display.height - top),
            original: params.get(params::TEXT).unwrap_or_default().to_string(),
            heading: String::new(),
            text: String::new(),
            lines: Vec::new(),
            top: 0,
            asked: false,
            needs_key: false,
        };
        panel.heading = panel.heading(env);
        let loading = env.i18n().tr("translate.loading").to_string();
        panel.set_text(env, loading);
        panel
    }

    /// Where the service's name begins in the header, from the left.
    fn source_x(&self, env: &Env<'_>) -> f32 {
        let source = env.i18n().tr("translate.source");
        let width = self
            .style
            .font(env)
            .map_or(0.0, |f| f.measure(self.style.small(), source));
        f32::from(self.header().width) - self.style.margin() as f32 - width
    }

    /// The original in one line, as wide as the header leaves it.
    fn heading(&self, env: &Env<'_>) -> String {
        let Some(font) = self.style.font(env) else {
            return String::new();
        };
        let original = self
            .original
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let room = self.source_x(env) - self.style.margin() as f32 * 2.0;
        ui::fit(&font, self.style.title(), &original, room)
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

    /// Sends the text off with `key`, or says what is missing.
    fn ask(&mut self, env: &Env<'_>, key: &str, cx: &mut Cx<'_, '_>) {
        self.needs_key = key.is_empty();
        let message = if !env.caps().network {
            Some("translate.no-network")
        } else if self.needs_key {
            Some("translate.needs-key")
        } else {
            None
        };
        if let Some(message) = message {
            let text = env.i18n().tr(message).to_string();
            self.set_text(env, text);
            cx.request_redraw();
            return;
        }
        let to = target(env.settings().read(&TO), env.i18n().language());
        cx.command(Command::http(request(key, &self.original, &to)));
    }

    /// Asks for the key on the keyboard.
    fn ask_key(&self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        cx.command(
            paperpaw_sdk::ask::text(env.i18n().tr("translate.key"), &KEY_ENTERED)
                .password()
                .command(),
        );
    }

    /// DeepL answered: the translation, or why not.
    fn answered(&mut self, env: &Env<'_>, reply: &HttpReply) {
        let text = reply.text();
        let body = text.as_ref();
        let key = match reply.status {
            200..=299 => match translation(body) {
                Some(text) => return self.set_text(env, text),
                None => "translate.failed",
            },
            401 | 403 => {
                self.needs_key = true;
                "translate.bad-key"
            }
            456 => "translate.quota",
            0 => "translate.unreachable",
            _ => "translate.failed",
        };
        let text = env.i18n().tr(key).to_string();
        self.set_text(env, text);
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
}

impl Screen for Panel {
    /// *Select* closes it, or asks for the key while there is none; the
    /// arrows scroll.
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM && !self.needs_key {
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
        // The original on the left, the service on the right.
        let header = self.header();
        let margin = style.margin() as f32;
        let source = env.i18n().tr("translate.source");
        let right = self.source_x(env);
        ui::Label::new(source)
            .px(style.small())
            .color(style.faint(ui::Ink::DETAIL))
            .draw_from(&mut ui, right, header);
        ui::Label::new(&self.heading)
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
        let key = env
            .secrets()
            .get(KEY.as_str())
            .unwrap_or_default()
            .to_string();
        self.ask(&env, &key, cx);
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(reply) = ANSWERED.read(event, params) {
            self.answered(&env, &reply);
        } else if let Some(typed) = KEY_ENTERED.read(event, params) {
            let key = typed.text.trim().to_string();
            if key.is_empty() {
                return;
            }
            cx.command(Command::set_secret(SecretChange::keep(
                KEY.as_str(),
                "translate.key",
                &key,
            )));
            let loading = env.i18n().tr("translate.loading").to_string();
            self.set_text(&env, loading);
            self.ask(&env, &key, cx);
        } else {
            return;
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
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
            if self.needs_key {
                self.ask_key(&env, cx);
            } else {
                cx.command(Command::Back);
            }
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
        if self.needs_key {
            self.ask_key(&env, cx);
            return Handled::Yes;
        }
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
    fn targets() {
        assert_eq!(target("translate.to.pt-br", "de"), "PT-BR");
        assert_eq!(target(AUTO, "de-AT"), "DE");
        assert_eq!(target(AUTO, "en"), "EN-GB");
        assert_eq!(target(AUTO, "xx"), "EN-GB");
    }

    #[test]
    fn requests_and_answers() {
        let free = request("abc:fx", "Hello world", "DE");
        assert_eq!(free.url(), "https://api-free.deepl.com/v2/translate");
        assert_eq!(free.body_text(), Some("text=Hello+world&target_lang=DE"));
        assert!(free.headers().contains(&(
            String::from("Authorization"),
            String::from("DeepL-Auth-Key abc:fx")
        )));
        assert_eq!(
            request("abc", "x", "DE").url(),
            "https://api.deepl.com/v2/translate"
        );
        let body =
            r#"{"translations":[{"detected_source_language":"EN","text":"Hallo \"Welt\""}]}"#;
        assert_eq!(translation(body).as_deref(), Some("Hallo \"Welt\""));
        assert_eq!(translation("{}"), None);
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
mod answer_tests;
