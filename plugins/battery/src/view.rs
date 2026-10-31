//! The view: the battery now, this time awake, the use on battery so far
//! (an hour awake, 100 pages, the hours left at that pace) and the recent
//! times awake, page by page where they do not fit.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Rect, RefreshMode, Screen,
};

use crate::log::{self, Kind, Reading, Session, Sessions};

/// A line of the view.
enum Line {
    Heading(String),
    Value(String, String),
    Message(String),
}

pub(crate) struct BatteryView {
    style: UiStyle,
    header: ui::Header,
    lines: Vec<Line>,
    /// The first line of each page, and the page shown.
    pages: Vec<usize>,
    page: usize,
    /// Where the log is, said at the bottom.
    footer: String,
}

/// Turn forward: the page buttons, and down or on where nothing else
/// moves.
const FORWARD: [Id; 4] = [
    actions::NEXT_PAGE,
    actions::DOWN,
    actions::NEXT,
    actions::RIGHT,
];
const BACKWARD: [Id; 4] = [
    actions::PREV_PAGE,
    actions::UP,
    actions::PREVIOUS,
    actions::LEFT,
];

/// `ms` as hours and minutes.
fn duration(env: &Env<'_>, ms: u64) -> String {
    let minutes = ms / 60_000;
    let (h, m) = ((minutes / 60) as i64, (minutes % 60) as i64);
    if minutes == 0 {
        env.i18n().tr("battery.under-a-minute").into()
    } else if h == 0 {
        env.i18n().format("battery.minutes", &[("m", Arg::Int(m))])
    } else {
        env.i18n()
            .format("battery.hours", &[("h", Arg::Int(h)), ("m", Arg::Int(m))])
    }
}

/// A share of the battery with one decimal, e.g. `4.2 %`.
fn percent(env: &Env<'_>, value: f32) -> String {
    let tenths = (value * 10.0 + 0.5) as u32;
    let point = env.i18n().tr("battery.decimal-point");
    let number = format!("{}{point}{}", tenths / 10, tenths % 10);
    env.i18n()
        .format("battery.percent", &[("value", Arg::Str(&number))])
}

/// The voltage, e.g. `4.01 V`.
fn volts(env: &Env<'_>, millivolts: u16) -> String {
    let point = env.i18n().tr("battery.decimal-point");
    // Rounded to hundredths first, so that 3999 mV is 4.00 V.
    let hundredths = (u32::from(millivolts) + 5) / 10;
    format!("{}{point}{:02} V", hundredths / 100, hundredths % 100)
}

/// Plugins listed in each section.
const PLUGINS_SHOWN: usize = 4;

/// A plugin's name; the kernel's work is PaperPaw's.
fn plugin_name(env: &Env<'_>, id: &str) -> String {
    match env.plugins().get(id) {
        Some(info) => String::from(info.name(env.i18n())),
        None if id == "core" => String::from(env.i18n().tr("battery.core")),
        None => String::from(id),
    }
}

/// What the reader set for `plugin`'s network, where it is not the
/// default: off, as in Normal while saving, only when asked.
fn network_setting(env: &Env<'_>, plugin: &str) -> String {
    let off = env.settings().get(&format!("core.network.{plugin}"))
        == Some(&paperpaw_sdk::SettingValue::Bool(false));
    if off {
        return String::from(env.i18n().tr("battery.network-off"));
    }
    match env.settings().text(&format!("core.saving.{plugin}")) {
        choice @ ("core.saving.normal" | "core.saving.manual") => {
            String::from(env.i18n().tr(choice))
        }
        _ => String::new(),
    }
}

/// Microseconds as seconds with a decimal, e.g. `3.2 s`.
fn seconds(env: &Env<'_>, micros: u64) -> String {
    let tenths = micros / 100_000;
    let point = env.i18n().tr("battery.decimal-point");
    format!("{}{point}{} s", tenths / 10, tenths % 10)
}

/// What a time awake used, for the list of recent ones.
fn session_line(env: &Env<'_>, s: &Session) -> Line {
    let pages = env
        .i18n()
        .plural("battery.pages-count", i64::from(s.pages()), &[]);
    let used = s.used().unwrap_or(0);
    Line::Value(
        format!("{} · {pages}", duration(env, s.millis())),
        env.i18n().format(
            "battery.used-percent",
            &[("value", Arg::Int(i64::from(used)))],
        ),
    )
}

impl BatteryView {
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub(crate) fn new(env: &Env<'_>, now: Reading, dir: &str) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let tr = |key: &str| String::from(env.i18n().tr(key));
        let mut sessions = Sessions::read(env.storage(), dir);
        // This time awake up to now.
        sessions.feed(Kind::Sample, now);
        let mut lines = Vec::new();

        lines.push(Line::Heading(tr("battery.now")));
        if let Some(charge) = now.percent {
            let mut value = env
                .i18n()
                .format("battery.percent", &[("value", Arg::Int(i64::from(charge)))]);
            if let Some(mv) = now.millivolts {
                value = format!("{value} · {}", volts(env, mv));
            }
            lines.push(Line::Value(tr("battery.charge"), value));
            let source = if now.charging {
                "battery.charging"
            } else {
                "battery.on-battery"
            };
            lines.push(Line::Value(tr("battery.power"), tr(source)));
        } else {
            lines.push(Line::Message(tr("battery.unknown")));
        }

        if let Some(current) = &sessions.current {
            lines.push(Line::Heading(tr("battery.awake")));
            lines.push(Line::Value(
                tr("battery.awake-for"),
                duration(env, current.millis()),
            ));
            if let (Some(from), Some(to)) = (current.first.percent, current.last.percent) {
                lines.push(Line::Value(
                    tr("battery.from-to"),
                    env.i18n().format(
                        "battery.percent-from-to",
                        &[
                            ("from", Arg::Int(i64::from(from))),
                            ("to", Arg::Int(i64::from(to))),
                        ],
                    ),
                ));
            }
            lines.push(Line::Value(
                tr("battery.pages"),
                format!("{}", current.pages()),
            ));
            lines.push(Line::Value(
                tr("battery.refreshes"),
                format!("{}", current.refreshes()),
            ));
        }

        lines.push(Line::Heading(tr("battery.so-far")));
        let totals = sessions.totals();
        match totals.per_hour() {
            Some(rate) => {
                lines.push(Line::Value(tr("battery.per-hour"), percent(env, rate)));
                if let Some(per_pages) = totals.per_100_pages() {
                    lines.push(Line::Value(
                        tr("battery.per-100-pages"),
                        percent(env, per_pages),
                    ));
                }
                if let Some(hours) = now.percent.and_then(|p| totals.hours_left(p)) {
                    lines.push(Line::Value(
                        tr("battery.left"),
                        env.i18n().format(
                            "battery.about-hours",
                            &[("h", Arg::Int((hours + 0.5) as i64))],
                        ),
                    ));
                }
            }
            None => lines.push(Line::Message(tr("battery.too-little"))),
        }

        // What each plugin took this time awake (its screens shown, its
        // code running): for developers.
        let developer = env
            .settings()
            .bool(paperpaw_sdk::builtin::settings::DEVELOPER.as_str())
            && env.settings().read(&crate::BY_PLUGIN);
        // Asked for only where shown (a copy of the kernel's table).
        let usage = if developer {
            env.plugin_usage()
        } else {
            Vec::new()
        };
        let mut shown: Vec<_> = usage
            .iter()
            .filter(|p| developer && p.shown_ms >= 60_000)
            .collect();
        shown.sort_by_key(|p| core::cmp::Reverse(p.shown_ms));
        if !shown.is_empty() {
            lines.push(Line::Heading(tr("battery.by-screen")));
            for p in shown.into_iter().take(PLUGINS_SHOWN) {
                let refreshes = i64::from(p.refreshes.total());
                lines.push(Line::Value(
                    plugin_name(env, p.plugin.as_str()),
                    env.i18n().plural(
                        "battery.shown-refreshes",
                        refreshes,
                        &[
                            ("time", Arg::Str(&duration(env, p.shown_ms))),
                            ("count", Arg::Int(refreshes)),
                        ],
                    ),
                ));
            }
        }
        let mut busy: Vec<_> = usage
            .iter()
            .filter(|p| developer && p.busy_us >= 100_000)
            .collect();
        busy.sort_by_key(|p| core::cmp::Reverse(p.busy_us));
        if !busy.is_empty() {
            lines.push(Line::Heading(tr("battery.by-processor")));
            for p in busy.into_iter().take(PLUGINS_SHOWN) {
                lines.push(Line::Value(
                    plugin_name(env, p.plugin.as_str()),
                    seconds(env, p.busy_us),
                ));
            }
        }

        // What each plugin did with the network this time awake, with what
        // the reader set for it and what its background work last said:
        // for everyone, so an exemption is never invisible.
        let net = env.plugin_usage();
        let mut net: Vec<_> = net.iter().filter(|p| p.requests + p.refused > 0).collect();
        net.sort_by_key(|p| core::cmp::Reverse(p.requests + p.refused));
        if !net.is_empty() {
            lines.push(Line::Heading(tr("battery.network")));
            for p in net.into_iter().take(PLUGINS_SHOWN) {
                let id = p.plugin.as_str();
                let value = env.i18n().plural(
                    "battery.requests",
                    i64::from(p.requests),
                    &[("count", Arg::Int(i64::from(p.requests)))],
                );
                lines.push(Line::Value(plugin_name(env, id), value));
                // Below: what was held back, what the reader set for it.
                let mut more = Vec::new();
                if p.refused > 0 {
                    more.push(env.i18n().plural(
                        "battery.refused",
                        i64::from(p.refused),
                        &[("count", Arg::Int(i64::from(p.refused)))],
                    ));
                }
                let set = network_setting(env, id);
                if !set.is_empty() {
                    more.push(set);
                }
                if !more.is_empty() {
                    lines.push(Line::Message(more.join(" · ")));
                }
                if let Ok(owner) = Id::parse(id)
                    && let Some(note) = env.work().note(&owner)
                {
                    lines.push(Line::Message(note.text.clone()));
                }
            }
        }

        let recent: Vec<&Session> = sessions.ended.iter().rev().filter(|s| s.counts()).collect();
        if !recent.is_empty() {
            lines.push(Line::Heading(tr("battery.recent")));
            lines.extend(recent.into_iter().map(|s| session_line(env, s)));
        }

        // Where the log is, for those who look into files: developers.
        let footer = if env
            .settings()
            .read(&paperpaw_sdk::builtin::settings::DEVELOPER)
        {
            env.i18n().format(
                "battery.log-file",
                &[("path", Arg::Str(&format!("{dir}/{}", log::FILE)))],
            )
        } else {
            String::new()
        };
        let mut view = Self {
            style,
            header,
            lines,
            pages: Vec::new(),
            page: 0,
            footer,
        };
        view.pages = view.paginate(env);
        view
    }

    /// The height of `line`.
    fn height(&self, env: &Env<'_>, line: &Line) -> u16 {
        match line {
            Line::Heading(_) => ui::heading_height(env, &self.style),
            Line::Value(..) | Line::Message(_) => self.style.row(),
        }
    }

    /// The room for lines: below the header, above the footer.
    fn room(&self, env: &Env<'_>) -> (u16, u16) {
        let top = self.header.area().y + self.header.area().height;
        let bottom = env.device().display.height.saturating_sub(self.style.row());
        (top, bottom)
    }

    /// The first line of each page; a heading last on a page goes to the
    /// next with what it heads.
    fn paginate(&self, env: &Env<'_>) -> Vec<usize> {
        let (top, bottom) = self.room(env);
        let mut pages = Vec::from([0]);
        let mut y = top;
        for (i, line) in self.lines.iter().enumerate() {
            let height = self.height(env, line);
            let start = pages.last().copied().unwrap_or(0);
            if y + height > bottom && i > start {
                let heading_last = i > start + 1 && matches!(self.lines[i - 1], Line::Heading(_));
                let first = if heading_last { i - 1 } else { i };
                pages.push(first);
                y = top;
                if heading_last {
                    y += self.height(env, &self.lines[first]);
                }
            }
            y += height;
        }
        pages
    }

    /// The page and the number of pages, where there are several.
    fn paging(&self) -> Option<ui::Paging> {
        (self.pages.len() > 1).then_some(ui::Paging::new(self.page, self.pages.len()))
    }
}

impl Screen for BatteryView {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        // Back, and the page buttons where there are pages.
        let turns = *action == actions::NEXT_PAGE || *action == actions::PREV_PAGE;
        if turns && self.pages.len() > 1 {
            paperpaw_sdk::Hint::Default
        } else {
            paperpaw_sdk::Hint::only_back(action)
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, env.i18n().tr("battery.title"), None, self.paging());
        let display = env.device().display;
        let footer_height = style.row();
        let (mut y, bottom) = self.room(env);
        let first = self.pages.get(self.page).copied().unwrap_or(0);
        let last = self
            .pages
            .get(self.page + 1)
            .copied()
            .unwrap_or(self.lines.len());
        for line in &self.lines[first..last] {
            let height = self.height(env, line);
            if y + height > bottom {
                break;
            }
            let rect = Rect::new(0, y, display.width, height);
            match line {
                Line::Heading(text) => ui.heading(rect, text),
                Line::Value(title, value) => ui::Row::value(title, value).draw(&mut ui, rect),
                Line::Message(text) => {
                    ui::Label::new(text)
                        .px(style.small())
                        .color(style.faint(ui::Ink::MUTED))
                        .draw_from(&mut ui, style.margin() as f32, rect);
                }
            }
            y += height;
        }
        let rect = Rect::new(0, bottom, display.width, footer_height);
        ui::Label::new(&self.footer)
            .px(style.small())
            .color(style.faint(ui::Ink::NOTE))
            .draw_from(&mut ui, style.margin() as f32, rect);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let page = if FORWARD.contains(action) {
            (self.page + 1).min(self.pages.len().saturating_sub(1))
        } else if BACKWARD.contains(action) {
            self.page.saturating_sub(1)
        } else {
            return Handled::No;
        };
        if page != self.page {
            self.page = page;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.paging();
        match self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            Some(action) => {
                cx.command(Command::run(action));
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
