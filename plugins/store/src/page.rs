//! A plugin's page: what it does, who made it, its licence, version and
//! size, for a code plugin that it is one and which PaperPaw it is made
//! for, and what can be done with it: install it, stop installing it,
//! update or remove it, restart where it waits for that. What the list
//! says shows at once; the plugin's own page at the library, with its
//! files, is fetched as the page opens.
//!
//! Before the first code plugin the page says once what code plugins are
//! (full access to the device, safe mode when one goes wrong). An update
//! signed by another author than the version before waits here, with a
//! warning, until the reader installs it anyway or not.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{self, Progress};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, HttpRequest, Id,
    MemoryClass, Params, Rect, RefreshMode, Screen, files,
};

use crate::catalog::{self, Device, Entry, Index, More, Plugin};
use crate::list::ENTRY;
use crate::room::Room;
use crate::{CATALOG, DOWNLOADED, INSTALLER, failed, queue, trust};

/// The answer to *Remove?*.
const REMOVE: Event<Picked> = Event::new("store.remove");
/// The answer with the plugin's page at the library.
const DETAILS: Event<HttpReply> = Event::new("store.details");

/// Lines of the summary at most.
const SUMMARY_LINES: usize = 8;

/// A plugin that takes more memory than this while it runs is said to be
/// tight on devices with little ([`MemoryClass::Small`]: the X4 keeps
/// about twice this free while the reader is open).
const TIGHT_HEAP: u64 = 32 * 1024;

/// Screenshots larger than these are not fetched on devices with little
/// memory: decoding them would take too much of it.
const SMALL_SHOT_SIDE: u32 = 1280;
const SMALL_SHOT_BYTES: u64 = 300 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Install,
    Stop,
    Update,
    Remove,
    Restart,
    /// The notice before the first code plugin: understood, install.
    Understood,
    /// Not now, after the notice or the warning.
    Cancel,
    /// Installed some other way: this store's version in its place.
    UseStore,
    /// Not installed: there is no room for it.
    NoRoom,
    /// Its screenshots, one after another.
    Shots,
    /// Where its source is, as a QR code.
    Source,
    /// How it works, at length, to read paged.
    How,
}

/// What the page shows above its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// The plugin: what it does, the facts.
    Plugin,
    /// The notice before the first code plugin.
    Notice,
}

/// The plugin's files, from its page at the library.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Details {
    /// Not asked for yet.
    None,
    /// Asked for, at this address.
    Asked(String),
    Here(Box<Plugin>),
    /// Not to be had.
    Failed,
}

pub(crate) struct PageScreen {
    style: UiStyle,
    header: ui::Header,
    /// Where the summary and the facts begin.
    top: u16,
    list: ListView,
    status: Rect,
    id: String,
    plugin: Option<Entry>,
    details: Details,
    /// *Install* or *Update* was chosen before the files were known.
    waiting: bool,
    /// The notice before the first code plugin is shown.
    notice: bool,
    /// The download going on, or the job waiting for the reader's word.
    job: Option<Progress>,
    /// The room left for plugins: read when the page opens and when
    /// plugins change, never while drawing.
    room: Room,
}

impl PageScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let line = (style.row()) * 3 / 2;
        let status = Rect::new(0, display.height - line, display.width, line);
        // The rows go below the text, where it ends (see `draw`).
        let area = Rect::new(0, top, display.width, status.y.saturating_sub(top));
        let id = params.get(params::ID).unwrap_or_default().to_string();
        let id_for_room = id.clone();
        // What the list says of it, while its own page is fetched.
        let plugin = params
            .get(ENTRY)
            .and_then(|n| n.parse().ok())
            .zip(Index::kept(env.storage()))
            .and_then(|(n, index)| index.entry(env.storage(), n))
            .filter(|plugin| plugin.id == id);
        Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            top,
            status,
            id,
            plugin,
            details: Details::None,
            waiting: false,
            notice: false,
            job: INSTALLER.progress(env.storage()),
            room: Room::of(env, Some(&id_for_room)),
            style,
            header,
        }
    }

    /// What the library says the plugin's code takes, where it says.
    fn image_len(&self) -> Option<u64> {
        match &self.details {
            Details::Here(details) => details.more.image_len,
            _ => None,
        }
    }

    /// The job of this plugin, if one goes on.
    fn own_job(&self) -> Option<&Progress> {
        self.job.as_ref().filter(|job| job.plugin == self.id)
    }

    /// What the page shows above its rows.
    fn shown(&self) -> Shown {
        if self.notice {
            Shown::Notice
        } else {
            Shown::Plugin
        }
    }

    fn rows(&self, env: &Env<'_>) -> Vec<Row> {
        let mut rows = self.action_rows(env);
        if self.shown() == Shown::Plugin
            && let Details::Here(plugin) = &self.details
        {
            if !plugin.more.description.is_empty() {
                rows.push(Row::How);
            }
            if !plugin.more.shots.is_empty() {
                rows.push(Row::Shots);
            }
            if !plugin.more.source.is_empty() {
                rows.push(Row::Source);
            }
        }
        rows
    }

    /// What can be done with the plugin.
    fn action_rows(&self, env: &Env<'_>) -> Vec<Row> {
        let Some(plugin) = &self.plugin else {
            return Vec::new();
        };
        match self.shown() {
            Shown::Notice => return alloc::vec![Row::Understood, Row::Cancel],
            Shown::Plugin => {}
        }
        if self.own_job().is_some() {
            return alloc::vec![Row::Stop];
        }
        let Some(info) = env.plugins().get(&plugin.id) else {
            if !self.room.fits(plugin, self.image_len()) {
                return alloc::vec![Row::NoRoom];
            }
            return alloc::vec![Row::Install];
        };
        // Not this one to install over, update or remove.
        if !catalog::may_install(env.plugins(), &plugin.id, plugin.code) {
            return Vec::new();
        }
        // A restart first where one is waited for; a code plugin removed
        // runs on until then, and offers nothing else.
        let mut rows = Vec::new();
        if info.restart_needed() {
            rows.push(Row::Restart);
        }
        if info.enabled {
            if plugin.newer(env) {
                rows.push(Row::Update);
            } else if plugin.local(env) {
                rows.push(Row::UseStore);
            }
            rows.push(Row::Remove);
        }
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Asks the library for the plugin's page.
    fn fetch(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(device) =
            Device::of(&env).filter(|_| env.caps().network && install::is_plugin_name(&self.id))
        else {
            self.details = Details::Failed;
            return;
        };
        let list = env.settings().read(&CATALOG);
        let language = env.i18n().language();
        let community = crate::community(&env);
        let url = catalog::plugin_url(list.trim(), device.name, language, &self.id, community);
        let mut request = HttpRequest::new("GET", &url, DETAILS);
        request = request.save_to(catalog::plugin_part(&self.id));
        request = request.with(Params::new().with(params::ID, self.id.clone()));
        cx.command(Command::http(request));
        self.details = Details::Asked(url);
    }

    fn fetched(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Details::Asked(url) = &self.details else {
            return;
        };
        let address = reply.final_url.clone().unwrap_or_else(|| url.clone());
        let fetched = catalog::plugin_part(&self.id);
        let read = Device::of(&env)
            .filter(|_| (200..300).contains(&reply.status))
            .and_then(|device| catalog::plugin(env.storage(), &fetched, &address, &device))
            // The page asked for, not another one's.
            .filter(|plugin| plugin.entry.id == self.id);
        let _ = env.storage().remove(&fetched);
        match read {
            Some(mut plugin) => {
                // Only screenshots this device can decode well.
                if env.device().memory == MemoryClass::Small {
                    plugin.more.shots.retain(|shot| {
                        shot.width <= SMALL_SHOT_SIDE
                            && shot.height <= SMALL_SHOT_SIDE
                            && shot.size <= SMALL_SHOT_BYTES
                    });
                }
                self.plugin = Some(plugin.entry.clone());
                self.details = Details::Here(Box::new(plugin));
                if core::mem::take(&mut self.waiting) {
                    self.install(cx);
                }
            }
            None => {
                self.details = Details::Failed;
                if core::mem::take(&mut self.waiting) {
                    let name = self.plugin.as_ref().map_or("", |p| p.name.as_str());
                    cx.command(failed(&env, name));
                }
            }
        }
        self.redraw(cx);
    }

    /// Installs the plugin (or its newer version), once its files are
    /// known and, before the first code plugin, the reader was told what
    /// code plugins are.
    fn install(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let name = self.plugin.as_ref().map_or("", |p| p.name.as_str());
        if !env.caps().network {
            let text = env.i18n().tr("store.offline").to_string();
            return cx.command(Command::Notify(text));
        }
        // One at a time: another one going on is said, not stopped.
        if let Some(other) = &self.job {
            let text = env
                .i18n()
                .format("store.busy", &[("name", Arg::Str(&other.name))]);
            return cx.command(Command::Notify(text));
        }
        let code = self.plugin.as_ref().is_some_and(|p| p.code);
        if code && !trust::told(env.storage()) {
            self.notice = true;
            self.list.set_focus(0);
            return self.redraw(cx);
        }
        match &self.details {
            Details::Here(plugin) => {
                let mut out = paperpaw_sdk::Commands::new();
                trust::expect(env.storage(), &plugin.entry.id, plugin.author_key.as_ref());
                if !INSTALLER.start(&env, &plugin.package, &mut out) {
                    out.push(failed(&env, name));
                }
                for command in out {
                    cx.command(command);
                }
                self.job = INSTALLER.progress(env.storage());
                self.list.set_focus(0);
                self.redraw(cx);
            }
            Details::Failed => cx.command(failed(&env, name)),
            Details::None | Details::Asked(_) => {
                self.waiting = true;
                if self.details == Details::None {
                    self.fetch(cx);
                }
                self.redraw(cx);
            }
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(plugin) = &self.plugin else {
            return;
        };
        match self.rows(&env).get(index).copied() {
            Some(Row::Install | Row::Update | Row::UseStore) => self.install(cx),
            Some(Row::NoRoom) => {
                if let Some(text) = self.room.needs(&env, plugin, self.image_len()) {
                    cx.command(Command::Notify(text));
                }
            }
            Some(Row::Understood) => {
                trust::tell(env.storage());
                self.notice = false;
                self.install(cx);
            }
            Some(Row::Cancel) => {
                self.notice = false;
                self.list.set_focus(0);
                self.redraw(cx);
            }
            Some(Row::Stop) => {
                INSTALLER.cancel(env.storage());
                // Only this one stops: the next update waiting goes on.
                let mut out = paperpaw_sdk::Commands::new();
                queue::next(&env, &mut out);
                for command in out {
                    cx.command(command);
                }
                self.job = INSTALLER.progress(env.storage());
                self.list.set_focus(0);
                self.redraw(cx);
            }
            Some(Row::Restart) => cx.command(Command::Restart),
            Some(Row::Shots) => {
                if let Details::Here(details) = &self.details {
                    cx.command(crate::shots::open(&details.more.shots));
                }
            }
            Some(Row::Source) => {
                if let Details::Here(details) = &self.details {
                    cx.command(crate::source::open(&plugin.name, &details.more.source));
                }
            }
            Some(Row::How) => {
                if let Details::Here(details) = &self.details {
                    let text = format!("{}\n\n{}\n", plugin.name, details.more.description);
                    let file = format!("{}/how.txt", Env::plugin_dir(crate::PLUGIN.as_str()));
                    match paperpaw_sdk::readers::show_text(env.storage(), &file, &text) {
                        Ok(open) => cx.command(open),
                        Err(_) => {
                            cx.command(Command::Notify(env.i18n().tr("store.how-failed").into()))
                        }
                    }
                }
            }
            Some(Row::Remove) => {
                let title = env
                    .i18n()
                    .format("store.remove-question", &[("name", Arg::Str(&plugin.name))]);
                cx.command(
                    paperpaw_sdk::ask::choice(&title, &REMOVE)
                        .about(&plugin.id)
                        .options("store.option.", &["remove", "keep"])
                        .command(),
                );
            }
            None => {}
        }
    }

    /// A row's name and what it says at its end.
    fn row_text(&self, env: &Env<'_>, plugin: &Entry, row: Row) -> (String, String) {
        let tr = |key: &str| env.i18n().tr(key).to_string();
        match row {
            Row::Install | Row::Update | Row::UseStore if self.waiting => {
                let name = match row {
                    Row::Install => "store.install",
                    Row::Update => "store.update",
                    _ => "store.use-store",
                };
                (tr(name), tr("store.preparing"))
            }
            Row::Install => (
                tr("store.install"),
                files::size(plugin.size, env.i18n().tr("store.decimal")),
            ),
            Row::Stop => {
                let job = self.job.clone().unwrap_or_default();
                (tr("store.stop"), format!("{} / {}", job.done, job.total))
            }
            Row::Update => (tr("store.update"), plugin.version.clone()),
            Row::UseStore => (tr("store.use-store"), plugin.version.clone()),
            Row::Remove => (tr("store.remove"), String::new()),
            Row::Restart => (tr("store.restart"), String::new()),
            Row::Understood => (tr("store.code.understood"), String::new()),
            Row::Cancel => (tr("store.not-now"), String::new()),
            Row::NoRoom => (
                tr("store.no-room"),
                files::size(plugin.size, env.i18n().tr("store.decimal")),
            ),
            Row::Shots => {
                let count = match &self.details {
                    Details::Here(details) => details.more.shots.len(),
                    _ => 0,
                };
                (tr("store.shots"), format!("{count}  ›"))
            }
            Row::Source => (tr("store.source"), String::from("›")),
            Row::How => (tr("store.how"), String::from("›")),
        }
    }

    /// What the page says above its rows: what the plugin does and the
    /// facts in small print, or the notice.
    fn texts(&self, env: &Env<'_>, plugin: &Entry) -> (String, Vec<(&'static str, String)>) {
        match self.shown() {
            Shown::Notice => (env.i18n().tr("store.code.notice").to_string(), Vec::new()),
            Shown::Plugin => {
                let more = match &self.details {
                    Details::Here(details) => Some(&details.more),
                    _ => None,
                };
                let room = (&self.room, self.image_len());
                (plugin.summary.clone(), facts(env, plugin, more, room))
            }
        }
    }
}

/// The facts below the summary, each with its icon: whether it has code
/// (and so full access) or is data alone, whether its build was checked
/// against its source, by whom and since when they publish, its version,
/// size and licence, which version is here.
fn facts(
    env: &Env<'_>,
    plugin: &Entry,
    more: Option<&More>,
    (room, image_len): (&Room, Option<u64>),
) -> Vec<(&'static str, String)> {
    let i18n = env.i18n();
    let mut facts = Vec::new();
    if plugin.code {
        facts.push((
            "key",
            i18n.format("store.kind.code", &[("sdk", Arg::Str(&plugin.sdk))]),
        ));
    } else {
        facts.push(("puzzle", i18n.tr("store.kind.data").to_string()));
    }
    match more.map(|m| m.verified.as_str()) {
        Some("verified") => facts.push(("circle-check", i18n.tr("store.verified").to_string())),
        Some("failed") => facts.push(("x", i18n.tr("store.verified.failed").to_string())),
        Some("unverified") => facts.push(("circle", i18n.tr("store.verified.not").to_string())),
        _ => {}
    }
    // Nobody reviewed it: said first, with where it comes from.
    if plugin.unreviewed {
        let repository = more.map_or("", |m| m.repository.trim_start_matches("https://"));
        let text = match repository {
            "" => i18n.tr("store.unreviewed.fact").to_string(),
            from => i18n.format("store.unreviewed.from", &[("repository", Arg::Str(from))]),
        };
        facts.insert(0, ("alert-triangle", text));
    }
    if plugin.official {
        facts.push(("paw", i18n.tr("store.official").to_string()));
    } else if !plugin.author.is_empty() {
        let by = match more.filter(|m| m.author_since > 0) {
            Some(more) => {
                let year = (more.author_since / 10000).to_string();
                i18n.plural(
                    "store.by-since",
                    i64::from(more.author_plugins),
                    &[
                        ("author", Arg::Str(&plugin.author)),
                        ("year", Arg::Str(&year)),
                    ],
                )
            }
            None => i18n.format("store.by", &[("author", Arg::Str(&plugin.author))]),
        };
        facts.push(("user", by));
    }
    // The key its code is signed with, which an update must have too: its
    // first 16 digits, as the library's page shows it whole.
    if plugin.code
        && !plugin.official
        && let Some(key) = more
            .map(|m| m.author_key.as_str())
            .filter(|k| k.len() == 64)
    {
        let groups: Vec<&str> = (0..4).map(|i| &key[i * 4..i * 4 + 4]).collect();
        let short = groups.join(" ");
        facts.push((
            "key",
            i18n.format("store.author-key", &[("key", Arg::Str(&short))]),
        ));
    }
    let size = files::size(plugin.size, i18n.tr("store.decimal"));
    let mut version = i18n.format(
        "store.version",
        &[
            ("version", Arg::Str(&plugin.version)),
            ("size", Arg::Str(&size)),
        ],
    );
    if !plugin.licence.is_empty() {
        version.push_str(" · ");
        version.push_str(&i18n.format("store.licence", &[("licence", Arg::Str(&plugin.licence))]));
    }
    facts.push(("info-circle", version));
    // The room it needs, and what is left.
    if plugin.installed(env).is_none()
        && let Some(needs) = room.needs(env, plugin, image_len)
    {
        facts.push(("stack-2", needs));
    }
    if let Some(heap) = more.and_then(|m| m.heap) {
        let size = files::size(heap, i18n.tr("store.decimal"));
        let small = env.device().memory == MemoryClass::Small;
        let key = if small && heap > TIGHT_HEAP {
            "store.heap.tight"
        } else {
            "store.heap"
        };
        facts.push(("chart-bar", i18n.format(key, &[("size", Arg::Str(&size))])));
    }
    if let Some(installed) = plugin.installed(env) {
        facts.push((
            "check",
            i18n.format(
                "store.installed-version",
                &[("version", Arg::Str(&installed.to_string()))],
            ),
        ));
    }
    facts
}

impl Screen for PageScreen {
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = self.rows(env);
        let title = self.plugin.as_ref().map_or("", |p| p.name.as_str());
        let Some(plugin) = &self.plugin else {
            let mut ui = ui::Ui::new(canvas, env, &style);
            self.header
                .draw(&mut ui, title, None, self.list.paging(rows.len()));
            let (icon, title, text) = match self.details {
                Details::Failed => ("cloud", "store.gone", "store.gone.help"),
                _ => ("cloud-download", "store.loading-plugin", ""),
            };
            let area = Rect::new(
                0,
                self.top,
                self.list.area().width,
                self.status.y.saturating_sub(self.top),
            );
            let text = if text.is_empty() {
                ""
            } else {
                env.i18n().tr(text)
            };
            ui.empty(area, icon, env.i18n().tr(title), text);
            return;
        };
        let Some(font) = style.font(env) else {
            return;
        };
        // What it does (or the notice, the warning), then the facts in
        // small print, then what can be done, each right below the other.
        // Laid out once without drawing, to place the rows (and so the
        // page number above them), then drawn the same way: where the
        // text ends and where the rows begin are one value.
        let margin = style.margin() as f32;
        let width = f32::from(self.list.area().width) - 2.0 * margin;
        let (text, facts) = self.texts(env, plugin);
        let room = f32::from(ui::icon_room(env, &style, true));
        let icon_size = style.small() * ui::ICON_SCALE;
        let line = font.line_metrics(style.small()).line_height;
        let top = f32::from(self.top);
        let full = self.list.area().width;
        // The text and the facts end where two rows still fit above the
        // note (the largest text on a small screen): the text shortened,
        // the facts that do not fit left out.
        let kept = rows.len().min(2) as f32 * f32::from(style.row());
        let bottom = f32::from(self.status.y.saturating_sub(style.gap())) - kept;
        let fit = |y: f32, size: f32| {
            ((bottom - y) / font.line_metrics(size).line_height).max(1.0) as usize
        };
        let content = |mut ui: Option<&mut ui::Ui<'_>>| -> f32 {
            let mut y = top;
            let paragraph = |ui: &mut Option<&mut ui::Ui<'_>>,
                             y: &mut f32,
                             text: &str,
                             size: f32,
                             color: Color,
                             most: usize,
                             indent: f32| {
                let metrics = font.line_metrics(size);
                let width = width - indent;
                let margin = margin + indent;
                let lines = font.wrap(size, text, width);
                let shown = lines.len().min(most);
                let Some(ui) = ui.as_deref_mut() else {
                    *y += metrics.line_height * shown as f32;
                    return;
                };
                let area = Rect::new(
                    margin as u16,
                    *y as u16,
                    width as u16,
                    (metrics.line_height * shown as f32) as u16,
                );
                ui.canvas().describe(Role::Text, text, area);
                for (n, range) in lines.iter().take(shown).enumerate() {
                    let text = if n + 1 == shown && lines.len() > shown {
                        ui::fit(&font, size, text[range.start..].trim_end(), width)
                    } else {
                        text[range.clone()].trim_end().to_string()
                    };
                    let baseline = *y + metrics.ascent;
                    font.draw(ui.canvas(), size, margin, baseline as i32, &text, color);
                    *y += metrics.line_height;
                }
            };
            let most = SUMMARY_LINES.min(fit(y, style.text()));
            paragraph(
                &mut ui,
                &mut y,
                &text,
                style.text(),
                Color::BLACK,
                most,
                0.0,
            );
            y += style.gap() as f32;
            // The facts each after its icon, their texts lined up.
            let ink = style.faint(ui::Ink::DETAIL);
            for (icon, fact) in &facts {
                let lines = font.wrap(style.small(), fact, width - room).len().min(3);
                if y + line * lines as f32 > bottom {
                    break;
                }
                if let Some(ui) = ui.as_deref_mut() {
                    let first = Rect::new(0, y as u16, full, line as u16);
                    ui.icon(icon, icon_size, margin, first, ink);
                }
                paragraph(&mut ui, &mut y, fact, style.small(), ink, 3, room);
                y += style.gap() as f32 / 3.0;
            }
            y
        };
        let gap = style.gap();
        let mut area = self.list.area();
        area.y = content(None) as u16 + gap;
        area.height = self.status.y.saturating_sub(area.y);
        self.list.set_area(area);
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(rows.len()));
        content(Some(&mut ui));
        let texts: Vec<(String, String)> = rows
            .iter()
            .map(|&row| self.row_text(env, plugin, row))
            .collect();
        self.list
            .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                let (name, value) = &texts[index];
                ui::Row::value(name, value).focused(focused).draw(ui, rect);
            });
        ui.canvas().fill(self.status, Color::WHITE);
        let note = match (&self.job, rows.first()) {
            (Some(job), Some(Row::Stop)) if job.plugin == plugin.id => {
                env.i18n().tr("store.downloading").to_string()
            }
            (Some(job), _) if job.plugin != plugin.id => env
                .i18n()
                .format("store.busy", &[("name", Arg::Str(&job.name))]),
            (_, Some(Row::Restart)) => env.i18n().tr("store.restart.help").to_string(),
            _ => String::new(),
        };
        if !note.is_empty() {
            ui.note_line(self.status, &note);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.job = INSTALLER.progress(env.storage());
        self.room = Room::of(&env, Some(&self.id));
        if self.details == Details::None {
            self.fetch(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows(cx.env()).len();
        if let Some(change) = self.list.navigate(action, len) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else if *action == actions::BACK && self.notice {
            // Back from the notice to the plugin.
            self.notice = false;
            self.redraw(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = self.rows(cx.env()).len();
        let paging = self.list.paging(len);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.activate(index, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = DETAILS.read(event, params) {
            if reply.context.get(params::ID) == Some(self.id.as_str()) {
                self.fetched(&reply, cx);
            }
        } else if let Some(picked) = REMOVE.read(event, params) {
            if picked.option == "remove"
                && let Ok(plugin) = Id::parse(&picked.path)
            {
                let env = *cx.env();
                let mut out = paperpaw_sdk::Commands::new();
                INSTALLER.remove(&env, &plugin, &mut out);
                let name = self.plugin.as_ref().map_or("", |p| p.name.as_str());
                out.push(Command::Notify(
                    env.i18n()
                        .format("store.removed", &[("name", Arg::Str(name))]),
                ));
                for command in out {
                    cx.command(command);
                }
                self.job = INSTALLER.progress(env.storage());
                self.list.set_focus(0);
            }
        } else if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            let env = *cx.env();
            self.job = INSTALLER.progress(env.storage());
            if *event == events::PLUGINS_CHANGED {
                self.room = Room::of(&env, Some(&self.id));
            }
            let len = self.rows(cx.env()).len();
            self.list.set_len(len);
            self.redraw(cx);
        }
    }
}
