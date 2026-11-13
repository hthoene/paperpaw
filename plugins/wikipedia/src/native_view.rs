//! The summary screen as a native overlay filling `core.lookup-online`: the
//! word over the lower part of the screen, its summary fetched through
//! `core.net`, scrolled by button or tap.
use crate::{
    declarations::FETCHED_TOKEN,
    native_model::{self, MAX_BODY, Outcome},
    native_page::{Page, Step},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        formatting::Arg,
        i18n,
        net::{BODY_PART, HttpDone, Net, REPLY, REQUEST_INPUT, Reason, Request, RequestId},
        snapshot,
        ui::{HandoverId, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, Paint, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint, text,
};

/// What the screen has to say.
#[derive(Debug, Default)]
enum Body {
    /// The summary is being fetched.
    #[default]
    Loading,
    /// Why there is none (the key of the text).
    Message(&'static str),
    /// The device is offline and the user's window to ask was missed: confirm
    /// (or a tap) goes online and asks again, back closes.
    Offer,
    /// The summary, with the article's title when the answer had one.
    Summary { title: Option<String>, text: String },
}

/// How the screen is laid out in a frame, from the last draw.
#[derive(Debug, Default, Clone, Copy)]
struct Layout {
    region: Rect,
    rows: usize,
}

/// One open summary screen.
#[derive(Debug, Default)]
pub struct View {
    word: String,
    language: String,
    body: Body,
    /// The request in flight.
    request: Option<RequestId>,
    /// Built at the first draw after the body changed, when the font is there
    /// to measure with.
    page: Option<Page>,
    layout: Layout,
}

impl View {
    /// The screen of the word and language in `handover`; the language is the
    /// interface's where there is none.
    ///
    /// # Errors
    /// The host's refusals; `NoMemory`.
    pub fn open(host: &Host<'_>, ui: &Ui, handover: Option<HandoverId>) -> Result<Self, Error> {
        let mut view = Self::default();
        let mut given = String::new();
        if let Some(handover) = handover {
            // A hand-over that cannot be read is a screen for no word, not a failed open:
            // the screen must exist for the draws that follow.
            let _ = read_params(
                host,
                ui,
                handover,
                &mut view.word,
                &mut view.language,
                &mut given,
            );
        }
        // The summary handed over (the dictionary panel's entry): not fetched again.
        if !given.trim().is_empty() {
            view.body = Body::Summary {
                title: None,
                text: given,
            };
        }
        view.word = clean(&view.word, MAX_WORD)?;
        view.language = clean(&view.language, MAX_LANGUAGE)?;
        if view.language.is_empty() {
            let mut scratch = [0u8; 64];
            view.language = text::copy(i18n::language(host, &mut scratch).unwrap_or("en"))?;
        }
        Ok(view)
    }

    /// Asks for the summary; where it cannot be asked, the screen says why.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn fetch(&mut self, host: &Host<'_>, net: &Net) -> Result<(), Error> {
        if self.word.trim().is_empty() {
            self.body = Body::Message("wikipedia.not-found");
            return Ok(());
        }
        match request_summary(host, net, &self.language, &self.word) {
            Ok(id) => self.request = Some(id),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => self.body = Body::Message("wikipedia.failed"),
        }
        Ok(())
    }

    /// Shows `text` (a summary the plugin has, or the one handed over),
    /// with the article's `title` where there is one: nothing is fetched.
    pub fn show(&mut self, title: Option<String>, text: String) {
        self.page = None;
        self.body = Body::Summary { title, text };
    }

    /// Whether the screen still has to fetch its summary.
    #[must_use]
    pub const fn is_loading(&self) -> bool {
        matches!(self.body, Body::Loading)
    }

    /// The word the screen is for.
    #[must_use]
    pub fn word(&self) -> &str {
        &self.word
    }

    /// Says why no request could be made at all (no network service).
    pub fn offline(&mut self) {
        self.body = Body::Message("wikipedia.no-network");
    }

    /// Whether `id` is the request this screen waits for.
    #[must_use]
    pub fn waits_for(&self, id: RequestId) -> bool {
        self.request == Some(id)
    }

    /// The request ended: the summary, or why there is none.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusals reading the body.
    pub fn done(&mut self, host: &Host<'_>, net: &Net, done: &HttpDone) -> Result<(), Error> {
        self.request = None;
        self.page = None;
        let reason = done.reason();
        self.body = match reason {
            Reason::Ok => match read_body(host, net, done) {
                Ok(body) => self.outcome(done.status(), &body)?,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => Body::Message("wikipedia.failed"),
            },
            Reason::Offline => Body::Offer,
            Reason::Saving | Reason::NetworkOff => Body::Message("wikipedia.no-network"),
            Reason::NoNetwork | Reason::Dns | Reason::Tls | Reason::Timeout | Reason::Refused => {
                Body::Message("wikipedia.unreachable")
            }
            _ => Body::Message("wikipedia.failed"),
        };
        Ok(())
    }

    fn outcome(&self, status: u32, body: &str) -> Result<Body, Error> {
        let status = u16::try_from(status).unwrap_or(u16::MAX);
        Ok(match native_model::outcome(status, body) {
            Ok(Outcome::Found { title, text }) => Body::Summary { title, text },
            Ok(Outcome::NotFound) => Body::Message("wikipedia.not-found"),
            Ok(Outcome::Unreachable) => Body::Message("wikipedia.unreachable"),
            Ok(_) | Err(Error::NoSpace | Error::Invalid) => Body::Message("wikipedia.failed"),
            Err(error) => return Err(error),
        })
    }

    /// What an input did: scrolls, or closes the screen.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
        net: Option<&Net>,
    ) -> Result<Handled, Error> {
        if matches!(self.body, Body::Offer) && wants_online(self.layout.region, input) {
            return self.go_online(host, net);
        }
        let Some(page) = self.page.as_mut() else {
            return Ok(Handled::No);
        };
        match page.decide(self.layout.rows, self.layout.region, input) {
            Step::Ignored => Ok(Handled::No),
            Step::Edge => Ok(Handled::Yes),
            Step::Scroll(top) => {
                page.set_top(top);
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
                Ok(Handled::Yes)
            }
            Step::Close => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).close_screen()?;
                Ok(Handled::Yes)
            }
        }
    }

    /// The user chose *Go online*: the network is asked for within the host's
    /// input window (this input), and the summary is asked for again.
    fn go_online(&mut self, host: &Host<'_>, net: Option<&Net>) -> Result<Handled, Error> {
        let mut scratch = [0; 64];
        let mut sender = CommandSender::new(host, &mut scratch);
        self.page = None;
        self.body = Body::Loading;
        match (sender.go_online(GO_ONLINE_MS), net) {
            (Ok(()), Some(net)) => self.fetch(host, net)?,
            (Err(Error::NoMemory), _) => return Err(Error::NoMemory),
            _ => self.body = Body::Message("wikipedia.no-network"),
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        Ok(Handled::Yes)
    }

    /// The name above the text: the article's title where the answer had one.
    fn heading(&self) -> &str {
        match &self.body {
            Body::Summary {
                title: Some(title), ..
            } => title,
            _ => &self.word,
        }
    }

    /// The text to wrap: the summary with its source and licence below it
    /// (CC BY-SA asks for both), or the message.
    fn shown(&self, host: &Host<'_>, messages: &mut Messages) -> Result<String, Error> {
        match &self.body {
            Body::Loading => messages.copy(host, "wikipedia.loading"),
            Body::Message(key) => messages.copy(host, key),
            Body::Offer => messages.copy(host, "wikipedia.offer"),
            Body::Summary { text, .. } => {
                let address = native_model::article_url(&self.language, self.heading())?;
                let from = messages.format(
                    host,
                    "wikipedia.source",
                    &[Arg::Text {
                        name: "address",
                        value: &address,
                    }],
                )?;
                text::concat(&[text, "\n\n", &from])
            }
        }
    }

    /// Draws the screen over the lower part of the display.
    ///
    /// # Errors
    /// Allocation, font, catalog and drawing errors; the layout of the last
    /// completed frame stays.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let font = Font::acquire_role(host, FontRole::Ui)?;
        // The font goes back on every path, an error too.
        let drawn = self.render(host, frame, &font);
        let released = font.release();
        drawn.and(released)
    }

    fn render(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        font: &Font<'_>,
    ) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let style = Style::read(host, &display, font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = height.saturating_mul(9) / 20;
        let region = Rect::new(0, top, width, height.saturating_sub(top));
        let line = paperpaw_ui_kit::geometry::line_px(font, style.text)?.max(1);
        let header = style.row;
        let room = region
            .height
            .saturating_sub(header)
            .saturating_sub(style.gap);
        let rows = usize::from(room / line).max(1);
        let mut messages = Messages::new()?;
        if self.page.is_none() {
            let shown = self.shown(host, &mut messages)?;
            let wide = f32::from(width.saturating_sub(2 * style.margin)).max(1.0);
            self.page = Some(Page::new(
                &shown,
                |t| font.measure_text(style.text, t),
                wide,
            )?);
        }
        let page = self.page.as_ref().ok_or(Error::State)?;
        let mut body = Vec::new();
        push(&mut body, Paint::Fill(region, style.paper))?;
        push(
            &mut body,
            Paint::Fill(
                Rect::new(region.x, region.y, region.width, style.line),
                style.rule,
            ),
        )?;
        let margin = f32::from(style.margin);
        let head = Rect::new(region.x, region.y, region.width, header);
        let source = messages.copy(host, "wikipedia.title")?;
        let source_width = font.measure_text(style.small, &source)?;
        let name = text::fit(
            font,
            style.title,
            self.heading(),
            f32::from(width) - 3.0 * margin - source_width,
        )?;
        text::label(
            &mut body,
            font,
            &name,
            style.title,
            margin,
            head,
            style.ink,
            None,
        )?;
        let x = f32::from(width) - margin - source_width;
        text::label(
            &mut body,
            font,
            &source,
            style.small,
            x,
            head,
            style.muted,
            None,
        )?;
        let mut y = region
            .y
            .saturating_add(header)
            .saturating_add(style.gap / 2);
        for row in page.page(rows) {
            let area = Rect::new(region.x, y, region.width, line);
            text::label(
                &mut body, font, row, style.text, margin, area, style.ink, None,
            )?;
            y = y.saturating_add(line);
        }
        paint::replay(frame, font, body.iter())?;
        self.layout = Layout { region, rows };
        Ok(())
    }
}

/// The id of the summary screen (what a provider's entry opens).
pub const SCREEN: &str = "wikipedia.lookup";

/// Asks `core.net` for the summary of `word`'s article in `language`
/// (`FETCHED_TOKEN`, at most [`MAX_BODY`]).
///
/// # Errors
/// `NoMemory`; the host's refusals.
pub fn request_summary(
    host: &Host<'_>,
    net: &Net,
    language: &str,
    word: &str,
) -> Result<RequestId, Error> {
    let url = native_model::summary_url(language, word)?;
    let headers = [
        paperpaw_sdk::native::net::Header::new("User-Agent", "PaperPaw/0.1 (e-reader)")?,
        paperpaw_sdk::native::net::Header::new("Accept", "application/json")?,
    ];
    let request = Request::get(FETCHED_TOKEN, &url)?
        .headers(&headers)?
        .max_body(MAX_BODY as u32);
    let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
    net.request(host, &request, &mut input, &mut out)
}

/// The body of a finished request, read in parts into one text; over
/// [`MAX_BODY`] or not UTF-8 is `NoSpace` / `Invalid` (no summary).
pub fn read_body(host: &Host<'_>, net: &Net, done: &HttpDone) -> Result<String, Error> {
    if done.truncated() || done.body_len() as usize > MAX_BODY {
        return Err(Error::NoSpace);
    }
    let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
    let mut bytes: Vec<u8> = Vec::new();
    bytes
        .try_reserve_exact(done.body_len() as usize)
        .map_err(|_| Error::NoMemory)?;
    loop {
        let offset = u32::try_from(bytes.len()).map_err(|_| Error::NoSpace)?;
        let part = net.body(host, done.id(), offset, &mut input, &mut out)?;
        if part.is_empty() {
            break;
        }
        if bytes.len() + part.len() > MAX_BODY {
            return Err(Error::NoSpace);
        }
        debug_assert!(part.len() <= BODY_PART);
        bytes.try_reserve(part.len()).map_err(|_| Error::NoMemory)?;
        bytes.extend_from_slice(part);
    }
    String::from_utf8(bytes).map_err(|_| Error::Invalid)
}

/// How long the network is asked for when the user chooses *Go online*.
const GO_ONLINE_MS: u32 = 120_000;

/// Whether `input` is the choice to go online: confirm, or a tap on the screen.
fn wants_online(region: Rect, input: &Input<'_>) -> bool {
    match *input {
        Input::Action { action, .. } => action == "core.confirm",
        Input::Gesture {
            position,
            kind: InputGestureKind::Tap,
            ..
        } => region.contains(position),
        _ => false,
    }
}

/// The most bytes of a word a request carries, and of a language.
const MAX_WORD: usize = 256;
const MAX_LANGUAGE: usize = 35;

/// `text` cut at a character to at most `max` bytes.
fn clean(text: &str, max: usize) -> Result<String, Error> {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text::copy(text.get(..end).unwrap_or(""))
}

/// The word, language and text of `handover`, as the caller gave them.
fn read_params(
    host: &Host<'_>,
    ui: &Ui,
    handover: HandoverId,
    word: &mut String,
    language: &mut String,
    given: &mut String,
) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    for param in ui.params(host, handover, &mut input, &mut out)? {
        let param = param?;
        match param.key() {
            "word" => *word = text::copy(param.value())?,
            "language" => *language = text::copy(param.value())?,
            "text" => *given = text::copy(param.value())?,
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_or_a_tap_on_the_screen_goes_online_and_nothing_else_does() {
        use paperpaw_sdk::Point;
        let region = Rect::new(0, 100, 200, 100);
        let action = |action| Input::Action { now_ms: 0, action };
        let tap = |x, y| Input::Gesture {
            now_ms: 0,
            position: Point::new(x, y),
            kind: InputGestureKind::Tap,
        };
        assert!(wants_online(region, &action("core.confirm")));
        assert!(!wants_online(region, &action("core.back")));
        assert!(!wants_online(region, &action("core.next-page")));
        assert!(wants_online(region, &tap(50, 150)));
        assert!(!wants_online(region, &tap(50, 50)));
    }

    #[test]
    fn a_long_word_is_cut_at_a_character() {
        let long = "é".repeat(1_000);
        let cut = clean(&long, MAX_WORD).unwrap();
        assert!(cut.len() <= MAX_WORD);
        assert!(cut.chars().all(|c| c == 'é'));
        assert_eq!(clean("cat", MAX_WORD).unwrap(), "cat");
    }
}
