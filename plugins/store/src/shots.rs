//! A plugin's screenshots, one at a time, turned like pages. Each is
//! fetched to the storage when shown and decoded there to the size of the
//! screen, so it takes no more memory than a row of it.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::image::{Fit, gray_from_file};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, HttpRequest, Id, Params, Rect,
    RefreshMode, Screen,
};

use crate::catalog::Shot;
use crate::{PLUGIN, SHOTS_SCREEN};

/// The answer with a screenshot.
const FETCHED: Event<HttpReply> = Event::new("store.shot");
/// Parameter of the answer: which screenshot it is.
const SHOT: &str = "shot";

/// The command that shows `shots`.
pub(crate) fn open(shots: &[Shot]) -> Command {
    let urls: Vec<&str> = shots.iter().map(|s| s.url.as_str()).collect();
    Command::OpenScreen(
        SHOTS_SCREEN,
        Params::new().with(params::TEXT, urls.join("\n")),
    )
}

fn file(name: &str) -> String {
    format!("{}/{name}", paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str()))
}

/// Where screenshot `at` is downloaded to.
fn part(at: usize) -> String {
    file(&format!("shot-{at}.part"))
}

/// What is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Picture {
    Loading,
    Here(ImageRef),
    Failed,
}

pub(crate) struct ShotsScreen {
    style: UiStyle,
    header: ui::Header,
    area: Rect,
    urls: Vec<String>,
    at: usize,
    picture: Picture,
}

impl ShotsScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let margin = style.margin();
        let area = Rect::new(
            margin,
            top,
            display.width.saturating_sub(2 * margin),
            display.height.saturating_sub(top + style.gap()),
        );
        let urls = params
            .get(params::TEXT)
            .unwrap_or_default()
            .lines()
            .map(ToString::to_string)
            .collect();
        Self {
            style,
            header,
            area,
            urls,
            at: 0,
            picture: Picture::Loading,
        }
    }

    fn fetch(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(url) = self.urls.get(self.at) else {
            return;
        };
        // Each in a file of its own, its answer known by its number: one
        // turned past while it came is not shown for another.
        let mut request = HttpRequest::new("GET", url, FETCHED);
        request = request.save_to(part(self.at));
        request = request.with(Params::new().with(SHOT, self.at.to_string()));
        cx.command(Command::http(request));
        self.picture = Picture::Loading;
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn turn(&mut self, by: isize, cx: &mut Cx<'_, '_>) {
        let at = self.at as isize + by;
        if at < 0 || at as usize >= self.urls.len() {
            return;
        }
        self.at = at as usize;
        self.fetch(cx);
    }
}

impl Screen for ShotsScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n().tr("store.shots");
        let paging = Some(ui::Paging::new(self.at, self.urls.len()));
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, title, None, paging);
        match &self.picture {
            Picture::Here(image) => {
                let (w, h) = ui::fit_image(image, self.area.width, self.area.height);
                let at = Rect::new(self.area.x + (self.area.width - w) / 2, self.area.y, w, h);
                ui::draw_image(ui.canvas(), env.storage(), image, at);
                // A thin frame: the screenshot's own paper apart from ours.
                ui.canvas().stroke_round(
                    at,
                    u32::from(style.line()).max(1),
                    0,
                    style.faint(ui::Ink::RULE),
                );
            }
            Picture::Loading => ui.empty(
                self.area,
                "cloud-download",
                env.i18n().tr("store.loading-plugin"),
                "",
            ),
            Picture::Failed => ui.empty(self.area, "cloud", env.i18n().tr("store.shot-failed"), ""),
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.picture == Picture::Loading {
            self.fetch(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if [
            actions::NEXT_PAGE,
            actions::RIGHT,
            actions::DOWN,
            actions::NEXT,
        ]
        .contains(a)
        {
            self.turn(1, cx);
        } else if [
            actions::PREV_PAGE,
            actions::LEFT,
            actions::UP,
            actions::PREVIOUS,
        ]
        .contains(a)
        {
            self.turn(-1, cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = Some(ui::Paging::new(self.at, self.urls.len()));
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        // Left half back, right half on, as in a book.
        let half = i32::from(cx.env().device().display.width) / 2;
        self.turn(if gesture.start.x < half { -1 } else { 1 }, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let Some(reply) = FETCHED.read(event, params) else {
            return;
        };
        let env = *cx.env();
        let Some(at) = reply
            .context
            .get(SHOT)
            .and_then(|at| at.parse::<usize>().ok())
        else {
            return;
        };
        let part = part(at);
        if at != self.at {
            // Turned past meanwhile: not this one's picture.
            let _ = env.storage().remove(&part);
            return;
        }
        let fit = Fit::screen(u32::from(self.area.width), u32::from(self.area.height));
        let image = (200..300)
            .contains(&reply.status)
            .then(|| gray_from_file(env.storage(), &part, &file("shot.gray"), fit))
            .flatten();
        let _ = env.storage().remove(&part);
        self.picture = image.map_or(Picture::Failed, Picture::Here);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Full);
    }
}
