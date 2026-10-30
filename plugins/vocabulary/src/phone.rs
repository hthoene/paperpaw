//! *Edit with your phone*: the reader serves a small page to the local
//! network and shows its address as a QR code; on the phone, decks and
//! cards are made, changed and deleted, and many cards come at once:
//! pasted as CSV or TSV, or from a deck exported by Anki (`.apkg`), read
//! by the phone's browser and sent as plain cards.
//!
//! The address carries a key drawn anew each time (see
//! [`paperpaw_sdk::served`]); the page is served only while the screen is
//! open. Its requests (all with the key, `k=`):
//!
//! - `GET /`: the page;
//! - `GET /api/decks`: `{"decks": [{"id", "name", "cards"}]}`;
//! - `GET /api/cards?deck=&from=`: `{"total", "cards": [{"id", "v",
//!   "front", "back", "example", "due", "reps"}]}`, [`PAGE`] at a time
//!   (`v` the card's [`version`](cards::Card::version));
//! - `POST /api/deck` (form: `name`, and `deck` to rename one):
//!   `{"id"}`; `POST /api/deck/delete?deck=`;
//! - `POST /api/card?deck=` (form: `front`, `back`, `example`, and `id`
//!   and `v` to change one); `POST /api/card/delete?deck=&id=&v=`: a card
//!   changed since `v` was sent is left as it is (409);
//! - `POST /api/import?deck=`: a card a line, `front<TAB>back<TAB>example`
//!   (tabs, line breaks and backslashes in a text written `\t`, `\n`,
//!   `\\`): `{"added"}`.
//!
//! A deck changed here is written at once; the answers of reviews are
//! folded into the decks before the page is served.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::qr::QrCode;
use paperpaw_sdk::replies::ServerNotice;
use paperpaw_sdk::served::{self, Access, Guard, json_string, param};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Instant, Params, Rect,
    RefreshMode, Screen, ServerResponse, Storage,
};

use crate::cards::{self, Problem, Texts};

pub(crate) const SCREEN: Id = Id::new_static("vocabulary.phone");
const SERVER: Id = Id::new_static("vocabulary.phone-server");
const SERVER_EVENT: Event<ServerNotice> = Event::new("vocabulary.phone-server-event");
/// The port served on.
const PORT: u16 = 8080;
/// Cards listed at a time, at most.
pub(crate) const PAGE: usize = 50;
/// Bytes of a page of cards, about: fewer cards where they are long.
const PAGE_BYTES: usize = 12 * 1024;
/// The longest form taken (a card's texts at their longest).
const MAX_FORM: usize = 16 * 1024;
/// The longest import taken, in bytes.
pub(crate) const MAX_IMPORT: u64 = 4 << 20;

/// The web page; its texts come from the translations.
const HTML: &str = include_str!("phone.html");

#[derive(Debug)]
enum State {
    Starting,
    Serving { url: String, qr: Option<QrCode> },
    Failed(String),
}

pub(crate) struct PhoneScreen {
    style: UiStyle,
    header: ui::Header,
    state: State,
    guard: Option<Guard>,
    started: bool,
    /// What the phone did last, to say below the code.
    last: Option<String>,
}

impl PhoneScreen {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        // The decks as they are: the answers given in them, the words
        // looked up as cards.
        let _ = cards::fold(env.storage());
        crate::take_lookups(env);
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            state: State::Starting,
            guard: None,
            started: false,
            last: None,
        }
    }

    fn content(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let top = self.header.area().height + self.style.gap();
        Rect::new(0, top, display.width, display.height.saturating_sub(top))
    }

    fn on_server_event(&mut self, notice: ServerNotice, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match notice {
            ServerNotice::Started { address, .. } => {
                self.state = match served::key(&env.random()) {
                    Some(key) => {
                        let url = format!("http://{address}/?k={key}");
                        self.guard = Some(Guard::new(key));
                        let qr = QrCode::new(url.as_bytes());
                        State::Serving { url, qr }
                    }
                    None => {
                        cx.command(Command::StopServing(SERVER));
                        State::Failed(env.i18n().tr("vocabulary.phone.no-random").into())
                    }
                };
            }
            ServerNotice::Failed { reason, .. } => {
                self.state = State::Failed(if reason == "no network" {
                    env.i18n().tr("vocabulary.phone.no-network").into()
                } else {
                    env.i18n()
                        .format("vocabulary.phone.failed", &[("reason", Arg::Str(&reason))])
                });
            }
            ServerNotice::Request {
                request: number,
                method,
                target,
                body_file,
                ..
            } => {
                let Some(guard) = &mut self.guard else {
                    return;
                };
                let request = Request {
                    method: &method,
                    target: &target,
                    body_file: body_file.as_deref(),
                    at: env.now(),
                };
                let page = || page(&env);
                let (response, done) = answer(number, guard, page, env.storage(), &request);
                cx.command(Command::respond(response));
                let Some(done) = done else {
                    return;
                };
                self.last = Some(done.text(&env));
            }
            _ => return,
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }
}

/// A request to the page.
pub(crate) struct Request<'a> {
    pub method: &'a str,
    pub target: &'a str,
    /// Where its body was saved, if it had one.
    pub body_file: Option<&'a str>,
    pub at: Instant,
}

/// What a request changed, to tell on the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Done {
    Deck(String),
    Card(String),
    Deleted,
    Imported(usize),
}

impl Done {
    fn text(&self, env: &Env<'_>) -> String {
        match self {
            Done::Deck(name) => env
                .i18n()
                .format("vocabulary.phone.deck-saved", &[("name", Arg::Str(name))]),
            Done::Card(front) => env
                .i18n()
                .format("vocabulary.phone.card-saved", &[("front", Arg::Str(front))]),
            Done::Deleted => env.i18n().tr("vocabulary.phone.deleted").into(),
            Done::Imported(n) => env
                .i18n()
                .plural("vocabulary.phone.imported", *n as i64, &[]),
        }
    }
}

const JSON: &str = "application/json";
const PLAIN: &str = "text/plain; charset=utf-8";

/// The answer to `request`, if `guard` lets it in, and what it changed;
/// `page` makes the page (only when it is asked for: it is not held).
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub(crate) fn answer(
    number: u32,
    guard: &mut Guard,
    page: impl FnOnce() -> Vec<u8>,
    storage: &dyn Storage,
    request: &Request<'_>,
) -> (ServerResponse, Option<Done>) {
    let respond =
        |status, kind: &str, body: &[u8]| ServerResponse::new(number, status, kind, body.to_vec());
    match guard.check_target(request.target, request.at) {
        Access::Granted => {}
        Access::Locked => {
            let text = b"Too many wrong keys: try again in a minute.";
            return (respond(429, PLAIN, text), None);
        }
        // Denied, and a state to come: refused as a wrong key is.
        _ => {
            let text = b"The key is missing or wrong: scan the code shown on the reader.";
            return (respond(403, PLAIN, text), None);
        }
    }
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((request.target, ""));
    let number_of = |name: &str| param(query, name).and_then(|n| n.parse::<u32>().ok());
    let form = || -> Option<String> {
        let file = request.body_file?;
        let bytes = paperpaw_sdk::read_file(storage, file, MAX_FORM).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };
    let result: Result<(String, Option<Done>), Problem> = match (request.method, path) {
        ("GET", "/") => {
            let html = "text/html; charset=utf-8";
            return (ServerResponse::new(number, 200, html, page()), None);
        }
        ("GET", "/api/decks") => Ok((decks_json(storage), None)),
        ("GET", "/api/cards") => match number_of("deck") {
            Some(deck) => {
                let from = number_of("from").unwrap_or(0) as usize;
                Ok((cards_json(storage, deck, from), None))
            }
            None => Err(Problem::Missing),
        },
        ("POST", "/api/deck") => {
            let form = form().unwrap_or_default();
            let name = param(&form, "name").unwrap_or_default();
            match param(&form, "deck").and_then(|d| d.parse().ok()) {
                Some(deck) => cards::rename_deck(storage, deck, &name).map(|()| deck),
                None => cards::add_deck(storage, &name),
            }
            .map(|id| {
                let name = cards::clean(&name, cards::MAX_NAME);
                (format!("{{\"id\":{id}}}"), Some(Done::Deck(name)))
            })
        }
        ("POST", "/api/deck/delete") => match number_of("deck") {
            Some(deck) => cards::delete_deck(storage, deck)
                .map(|()| (String::from("{}"), Some(Done::Deleted))),
            None => Err(Problem::Missing),
        },
        ("POST", "/api/card") => match number_of("deck") {
            Some(deck) => {
                let form = form().unwrap_or_default();
                let field = |name: &str| param(&form, name).unwrap_or_default();
                let texts = Texts {
                    front: field("front"),
                    back: field("back"),
                    example: field("example"),
                };
                let front = cards::clean(&texts.front, cards::MAX_TEXT);
                let seen = param(&form, "v").and_then(|v| v.parse().ok());
                match param(&form, "id").and_then(|i| i.parse().ok()) {
                    Some(id) => cards::edit(storage, deck, id, seen, &texts),
                    None => match cards::add(storage, deck, [texts]) {
                        Ok(0) => Err(Problem::Empty),
                        Ok(_) => Ok(()),
                        Err(e) => Err(e),
                    },
                }
                .map(|()| (String::from("{}"), Some(Done::Card(front))))
            }
            None => Err(Problem::Missing),
        },
        ("POST", "/api/card/delete") => match (number_of("deck"), number_of("id")) {
            (Some(deck), Some(id)) => cards::delete(storage, deck, id, number_of("v"))
                .map(|()| (String::from("{}"), Some(Done::Deleted))),
            _ => Err(Problem::Missing),
        },
        ("POST", "/api/import") => match (number_of("deck"), request.body_file) {
            (Some(_), Some(file)) if storage.size(file).is_ok_and(|s| s > MAX_IMPORT) => {
                let text = b"Too many cards at once for the reader: send them in parts.";
                return (respond(413, PLAIN, text), None);
            }
            (Some(deck), Some(file)) => cards::add_from(storage, deck, file)
                .map(|n| (format!("{{\"added\":{n}}}"), Some(Done::Imported(n)))),
            (Some(_), None) => Ok((String::from("{\"added\":0}"), None)),
            _ => Err(Problem::Missing),
        },
        _ => return (respond(404, PLAIN, b"not found"), None),
    };
    match result {
        // The body as made, not copied (a page of cards is large for a
        // small device's memory).
        Ok((body, done)) => (
            ServerResponse::new(number, 200, JSON, body.into_bytes()),
            done,
        ),
        Err(problem) => {
            let (status, text): (u16, &[u8]) = match problem {
                Problem::Missing => (404, b"No such deck or card: reload the page."),
                Problem::Full => (409, b"This deck or the reader is full: make another deck."),
                Problem::Changed => (409, b"The card was changed meanwhile: reload the page."),
                Problem::Empty => (422, b"A name or a front is needed."),
                Problem::Storage(_) => (500, b"The reader could not save it."),
            };
            (respond(status, PLAIN, text), None)
        }
    }
}

fn decks_json(storage: &dyn Storage) -> String {
    let sizes = cards::sizes(storage);
    let decks: Vec<String> = cards::decks(storage)
        .unwrap_or_default()
        .iter()
        .map(|d| {
            format!(
                "{{\"id\":{},\"name\":{},\"cards\":{}}}",
                d.id,
                json_string(&d.name),
                sizes.get(&d.id).copied().unwrap_or(0)
            )
        })
        .collect();
    format!("{{\"decks\":[{}]}}", decks.join(","))
}

/// A page of `deck`'s cards from number `from`: at most [`PAGE`] cards
/// and about [`PAGE_BYTES`], written straight into the answer.
fn cards_json(storage: &dyn Storage, deck: u32, from: usize) -> String {
    let mut out = String::from("{\"cards\":[");
    let mut shown = 0;
    let total = cards::visit_page(storage, deck, from, |c| {
        if shown > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"id\":{},\"v\":{},\"front\":",
            c.state.id,
            c.version()
        ));
        out.push_str(&json_string(&c.front));
        out.push_str(",\"back\":");
        out.push_str(&json_string(&c.back));
        out.push_str(",\"example\":");
        out.push_str(&json_string(&c.example));
        out.push_str(&format!(
            ",\"due\":{},\"reps\":{}}}",
            c.state.due, c.state.reps
        ));
        shown += 1;
        shown < PAGE && out.len() < PAGE_BYTES
    });
    out.push_str(&format!("],\"total\":{total}}}"));
    out
}

/// The keys of the page's texts, as it names them.
const TEXTS: [(&str, &str); 37] = [
    ("title", "vocabulary.web.title"),
    ("decks", "vocabulary.web.decks"),
    ("newDeck", "vocabulary.web.new-deck"),
    ("deckName", "vocabulary.web.deck-name"),
    ("add", "vocabulary.web.add"),
    ("save", "vocabulary.web.save"),
    ("cancel", "vocabulary.web.cancel"),
    ("rename", "vocabulary.web.rename"),
    ("delete", "vocabulary.web.delete"),
    ("deleteDeck", "vocabulary.web.delete-deck"),
    ("deleteCard", "vocabulary.web.delete-card"),
    ("cards", "vocabulary.web.cards"),
    ("noCards", "vocabulary.web.no-cards"),
    ("noDecks", "vocabulary.web.no-decks"),
    ("front", "vocabulary.web.front"),
    ("back", "vocabulary.web.back"),
    ("example", "vocabulary.web.example"),
    ("newCard", "vocabulary.web.new-card"),
    ("edit", "vocabulary.web.edit"),
    ("more", "vocabulary.web.more"),
    ("import", "vocabulary.web.import"),
    ("importHelp", "vocabulary.web.import-help"),
    ("paste", "vocabulary.web.paste"),
    ("header", "vocabulary.web.header"),
    ("file", "vocabulary.web.file"),
    ("send", "vocabulary.web.send"),
    ("read", "vocabulary.web.read"),
    ("imported", "vocabulary.web.imported"),
    ("failed", "vocabulary.web.failed"),
    ("newerAnki", "vocabulary.web.newer-anki"),
    ("noNotes", "vocabulary.web.no-notes"),
    ("tooLarge", "vocabulary.web.too-large"),
    ("wrongType", "vocabulary.web.wrong-type"),
    ("notUtf8", "vocabulary.web.not-utf8"),
    ("noCards", "vocabulary.web.no-cards"),
    ("partial", "vocabulary.web.partial"),
    ("full", "vocabulary.web.full"),
];

/// The page with its texts in the reader's language and its limits, made
/// in one pass (the page is large for a small device's memory).
fn page(env: &Env<'_>) -> Vec<u8> {
    let mut json = String::from("{");
    for (i, (name, key)) in TEXTS.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("{name}:{}", json_string(env.i18n().tr(key))));
    }
    json.push('}');
    let filled = [
        ("/*STRINGS*/{}", json),
        ("/*MAX_IMPORT*/0", MAX_IMPORT.to_string()),
        ("/*MAX_TEXT*/0", cards::MAX_TEXT.to_string()),
    ];
    let mut out = Vec::with_capacity(HTML.len() + filled[0].1.len());
    let mut rest = HTML;
    for (mark, value) in &filled {
        if let Some((before, after)) = rest.split_once(mark) {
            out.extend_from_slice(before.as_bytes());
            out.extend_from_slice(value.as_bytes());
            rest = after;
        }
    }
    out.extend_from_slice(rest.as_bytes());
    out
}

impl Screen for PhoneScreen {
    fn hint(&self, action: &Id, _: &Env<'_>) -> Hint {
        // Only back does anything here.
        Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n().tr("vocabulary.phone.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, title, None, None);
        let area = self.content(env);
        match &self.state {
            State::Starting => ui.message(area, env.i18n().tr("vocabulary.phone.starting")),
            State::Failed(message) => ui.message(area, message),
            State::Serving { url, qr } => {
                let hint = env.i18n().tr("vocabulary.phone.hint");
                ui::draw_address(
                    ui.canvas(),
                    env,
                    &style,
                    area,
                    url,
                    qr.as_ref(),
                    hint,
                    self.last.as_deref(),
                );
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.started {
            self.started = true;
            if !cx.env().random().available() {
                self.state = State::Failed(cx.env().i18n().tr("vocabulary.phone.no-random").into());
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                return;
            }
            cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            cx.command(Command::Back);
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(notice) = SERVER_EVENT.read(event, params)
            && *notice.server() == SERVER
        {
            self.on_server_event(notice, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    const KEY: &str = "abc";

    fn ask(
        storage: &MemoryStorage,
        method: &str,
        target: &str,
        body: Option<&str>,
    ) -> (u16, String, Option<Done>) {
        if let Some(body) = body {
            storage.write("body", body.as_bytes()).unwrap();
        }
        let mut guard = Guard::new(KEY.into());
        let request = Request {
            method,
            target,
            body_file: body.map(|_| "body"),
            at: Instant(0),
        };
        let page = || b"<page>".to_vec();
        let (response, done) = answer(1, &mut guard, page, storage, &request);
        let body = String::from_utf8_lossy(response.body()).into_owned();
        (response.status(), body, done)
    }

    /// The import's texts (en and de) name the most cards a deck holds.
    #[test]
    fn the_import_texts_name_the_deck_limit() {
        assert_eq!(cards::MAX_CARDS, 20_000);
        for (text, number) in [
            (include_str!("../i18n/en.ini"), "20,000"),
            (include_str!("../i18n/de.ini"), "20.000"),
        ] {
            for key in ["web.import-help", "web.partial", "web.full"] {
                let line = text
                    .lines()
                    .find(|l| l.starts_with(&format!("vocabulary.{key} ")))
                    .unwrap();
                assert!(line.contains(number), "{key}: {line}");
            }
        }
    }

    /// The page's marks are filled in one pass, in the order they come.
    #[test]
    fn the_page_has_its_marks_in_order() {
        let at = |mark: &str| HTML.find(mark).unwrap();
        assert!(at("/*STRINGS*/{}") < at("/*MAX_IMPORT*/0"));
        assert!(at("/*MAX_IMPORT*/0") < at("/*MAX_TEXT*/0"));
    }

    #[test]
    fn decks_and_cards_are_made_on_the_phone() {
        let storage = MemoryStorage::new();
        assert_eq!(ask(&storage, "GET", "/?k=abc", None).0, 200);
        assert_eq!(ask(&storage, "GET", "/?k=wrong", None).0, 403);
        assert_eq!(ask(&storage, "GET", "/api/decks", None).0, 403);
        let (status, body, done) = ask(
            &storage,
            "POST",
            "/api/deck?k=abc",
            Some("name=W%C3%B6rter"),
        );
        assert_eq!((status, body.as_str()), (200, "{\"id\":1}"));
        assert_eq!(done, Some(Done::Deck("Wörter".into())));
        let card = "front=Hund&back=dog&example=Der+Hund+bellt.";
        assert_eq!(
            ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(card)).0,
            200
        );
        assert_eq!(
            ask(&storage, "POST", "/api/card?k=abc&deck=1", Some("back=x")).0,
            422
        );
        assert_eq!(
            ask(&storage, "POST", "/api/card?k=abc&deck=9", Some(card)).0,
            404
        );
        let import = "Katze\tcat\nMaus\tmouse\tDie Maus.\n";
        let (status, body, done) = ask(&storage, "POST", "/api/import?k=abc&deck=1", Some(import));
        assert_eq!(
            (status, body.as_str(), done),
            (200, "{\"added\":2}", Some(Done::Imported(2)))
        );
        let (_, decks, _) = ask(&storage, "GET", "/api/decks?k=abc", None);
        assert_eq!(
            decks,
            "{\"decks\":[{\"id\":1,\"name\":\"Wörter\",\"cards\":3}]}"
        );
        let (_, list, _) = ask(&storage, "GET", "/api/cards?k=abc&deck=1&from=2", None);
        assert!(list.starts_with("{\"cards\":[{\"id\":3,\"v\":"), "{list}");
        assert!(list.ends_with("],\"total\":3}"), "{list}");

        let edit = "id=1&front=Hund&back=the+dog";
        assert_eq!(
            ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(edit)).0,
            200
        );
        assert_eq!(cards::page(&storage, 1, 0, 1).0[0].back, "the dog");
        assert_eq!(
            ask(&storage, "POST", "/api/card/delete?k=abc&deck=1&id=2", None).0,
            200
        );
        assert_eq!(
            ask(&storage, "POST", "/api/card/delete?k=abc&deck=1&id=2", None).0,
            404
        );
        assert_eq!(
            ask(
                &storage,
                "POST",
                "/api/deck?k=abc",
                Some("deck=1&name=Words")
            )
            .0,
            200
        );
        assert_eq!(cards::decks(&storage).unwrap()[0].name, "Words");
        assert_eq!(
            ask(&storage, "POST", "/api/deck/delete?k=abc&deck=1", None).0,
            200
        );
        assert!(cards::decks(&storage).unwrap().is_empty());
        assert_eq!(ask(&storage, "GET", "/nothing?k=abc", None).0, 404);
    }

    /// Two tabs open: one deletes the newest card and the other adds one.
    /// What the first then sends for the card it deleted finds nothing,
    /// and a change from texts it saw before another tab changed them is
    /// refused: neither card is touched.
    #[test]
    fn a_stale_tab_cannot_change_a_card_it_never_saw() {
        let storage = MemoryStorage::new();
        ask(&storage, "POST", "/api/deck?k=abc", Some("name=Words"));
        for card in ["front=Hund&back=dog", "front=Katze&back=cat"] {
            ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(card));
        }
        let (_, list, _) = ask(&storage, "GET", "/api/cards?k=abc&deck=1", None);
        let version = |n: usize| cards::page(&storage, 1, n, 1).0[0].version();
        let (dog, cat) = (version(0), version(1));
        assert!(list.contains(&format!("\"id\":2,\"v\":{cat},")), "{list}");

        let delete = format!("/api/card/delete?k=abc&deck=1&id=2&v={cat}");
        assert_eq!(ask(&storage, "POST", &delete, None).0, 200);
        let mouse = "front=Maus&back=mouse";
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(mouse));
        let stale = format!("id=2&v={cat}&front=Katze&back=the+cat");
        let status = ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&stale)).0;
        assert_eq!(status, 404);
        assert_eq!(ask(&storage, "POST", &delete, None).0, 404);
        let (cards, total) = cards::page(&storage, 1, 0, 9);
        assert_eq!((total, cards[1].front.as_str()), (2, "Maus"));
        assert_eq!(cards[1].back, "mouse");

        let fresh = format!("id=1&v={dog}&front=Hund&back=the+dog");
        assert_eq!(
            ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&fresh)).0,
            200
        );
        let stale = format!("id=1&v={dog}&front=Hund&back=a+dog");
        let (status, body, done) = ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&stale));
        assert_eq!((status, done), (409, None), "{body}");
        let delete = format!("/api/card/delete?k=abc&deck=1&id=1&v={dog}");
        assert_eq!(ask(&storage, "POST", &delete, None).0, 409);
        assert_eq!(cards::page(&storage, 1, 0, 1).0[0].back, "the dog");
    }
}
