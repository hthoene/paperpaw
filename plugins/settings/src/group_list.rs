//! The list of setting groups, the first screen of the settings: a
//! group's page, or a plugin's settings, chosen from it.

use super::*;

// ---- Main page: groups -------------------------------------------------

/// What an entry of the main page opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Link {
    Group(Id),
    /// A page of another plugin: its action.
    Page(Id),
    /// The switch for advanced settings.
    Advanced,
    /// Searching the settings.
    Search,
    /// Resetting all settings to their defaults.
    Reset,
    /// Restarting PaperPaw, once asked.
    Restart,
}

#[derive(Debug)]
pub(super) struct Entry {
    /// `None` for a section's heading.
    pub(super) link: Option<Link>,
    pub(super) title: String,
    pub(super) icon: Option<String>,
}

#[derive(Debug)]
pub(super) struct GroupList {
    /// Groups with visible settings and pages of other plugins, by order
    /// under the headings of their sections, then the advanced switch.
    pub(super) entries: Vec<Entry>,
    pub(super) style: UiStyle,
    pub(super) header: ui::Header,
    pub(super) rows: ui::Rows,
    /// The focus shows (buttons were used).
    pub(super) keys: bool,
    /// Room for icons at the start of rows.
    pub(super) icon_room: u16,
    /// Covered by a full screen, and settings changed meanwhile.
    pub(super) covered: Covered,
}

/// Where a group that names no order is listed: at the end of the section
/// for customising PaperPaw (from 201), after those that name one, by
/// name.
const UNORDERED: i32 = 10_000;

/// The icon of a group that names none.
const GROUP_ICON: &str = "puzzle";

/// The groups with settings to show, as the main page places them: by
/// their order, a group that names none at the end of the section for
/// customising, by name; each with its icon, or a plugin's.
fn groups(env: &Env<'_>) -> Vec<(i32, bool, Entry)> {
    let holding = groups_holding(env, |spec| visible(env, spec));
    let mut found: Vec<(i32, bool, Entry)> = env
        .settings()
        .groups()
        // A widget's own settings are opened where it is edited; a part of
        // a group on that group's page.
        .filter(|group| !env.widgets().has(group.id.as_str()) && group.parent.is_none())
        .filter(|group| {
            holding
                .iter()
                .any(|held| env.settings().within(held, &group.id))
        })
        .map(|group| {
            let order = if group.order == 0 {
                UNORDERED
            } else {
                group.order
            };
            let icon = env.menus().icon(group.id.as_str()).unwrap_or(GROUP_ICON);
            (
                order,
                true,
                Entry {
                    link: Some(Link::Group(group.id.clone())),
                    title: env.i18n().tr(&group.title).to_string(),
                    icon: Some(String::from(icon)),
                },
            )
        })
        .collect();
    // By name where the order is the same; the main page's sort keeps it.
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.title.cmp(&b.2.title)));
    found
}

impl GroupList {
    pub(super) fn new(env: &Env<'_>) -> Self {
        let settings_menu = menus::SETTINGS;
        let menu = settings_menu.as_str();
        // (order, headings before entries of the same order, entry)
        let mut placed: Vec<(i32, bool, Entry)> = env
            .menus()
            .items(menu)
            .iter()
            // Passwords and keys only once there are some.
            .filter(|item| *item.id != secrets::OPEN || env.secrets().listed().next().is_some())
            .map(|item| {
                let order = env.menus().order_of(menu, item.id.as_str());
                (
                    order.map_or(i32::from(i16::MAX), i32::from),
                    !item.section,
                    Entry {
                        link: (!item.section).then(|| Link::Page(item.id.clone())),
                        title: env.i18n().tr(item.title).to_string(),
                        icon: item.icon.map(String::from),
                    },
                )
            })
            .collect();
        placed.extend(groups(env));
        placed.push((
            i32::MIN,
            true,
            Entry {
                link: Some(Link::Search),
                title: env.i18n().tr("settings.search").to_string(),
                icon: Some(String::from("search")),
            },
        ));
        placed.push((
            i32::MAX,
            true,
            Entry {
                link: Some(Link::Advanced),
                title: env.i18n().tr("settings.advanced").to_string(),
                icon: Some(String::from("eye")),
            },
        ));
        placed.push((
            i32::MAX,
            true,
            Entry {
                link: Some(Link::Reset),
                title: env.i18n().tr("settings.reset-all").to_string(),
                icon: Some(String::from("refresh")),
            },
        ));
        // Last: what plugins saved goes along, as when the device goes off.
        placed.push((
            i32::MAX,
            true,
            Entry {
                link: Some(Link::Restart),
                title: env.i18n().tr("settings.restart").to_string(),
                icon: Some(String::from("power")),
            },
        ));
        placed.sort_by_cached_key(|(order, entry, _)| (*order, *entry));
        let mut entries: Vec<Entry> = placed.into_iter().map(|(_, _, entry)| entry).collect();
        // Headings with nothing under them go.
        let empty: Vec<bool> = (0..entries.len())
            .map(|i| {
                entries[i].link.is_none()
                    && entries.get(i + 1).is_none_or(|next| next.link.is_none())
            })
            .collect();
        let mut index = 0;
        entries.retain(|_| {
            index += 1;
            !empty[index - 1]
        });
        let (style, header, rows, icon_room) = Self::fitted(env, &entries);
        Self {
            entries,
            style,
            header,
            rows,
            keys: ui::focus_shown(env),
            icon_room,
            covered: Covered::default(),
        }
    }

    /// The parts placed for `env`'s screen: the style, the header, the rows
    /// of `entries` and the room for icons.
    fn fitted(env: &Env<'_>, entries: &[Entry]) -> (UiStyle, ui::Header, ui::Rows, u16) {
        let (style, header, area) = page(env);
        let heading = ui::heading_height(env, &style);
        let heights = entries
            .iter()
            .map(|e| match e.link {
                None => heading,
                Some(Link::Search) => ui::SearchField::height(&style),
                Some(_) => style.row(),
            })
            .collect();
        let headings = entries.iter().map(|e| e.link.is_none()).collect();
        let icon_room = ui::icon_room(
            env,
            &style,
            entries.iter().any(|e| e.link.is_some() && e.icon.is_some()),
        );
        let rows = ui::Rows::new(area, heights, headings);
        (style, header, rows, icon_room)
    }

    /// Makes the list anew (language, UI size or the advanced switch may
    /// have changed), keeping its focus.
    fn rebuild(&mut self, env: &Env<'_>) {
        let (focus, keys) = (self.rows.focus(), self.keys);
        *self = Self::new(env);
        self.keys = keys;
        refocus(&mut self.rows, focus);
    }

    pub(super) fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        match self.entries.get(index).and_then(|e| e.link.as_ref()) {
            Some(Link::Group(id)) => {
                // The group's own page, if its plugin brings one.
                let page = cx
                    .env()
                    .settings()
                    .groups()
                    .find(|g| g.id == *id)
                    .and_then(|g| g.page.clone());
                cx.command(Command::OpenScreen(
                    page.unwrap_or(GROUP_SCREEN),
                    Params::new().with(ID, id.as_str().to_string()),
                ));
            }
            Some(Link::Page(action)) => cx.command(Command::run(action.clone())),
            Some(Link::Advanced) => {
                let advanced = cx.env().settings().read(&ADVANCED);
                cx.command(Command::set(&ADVANCED, !advanced));
            }
            Some(Link::Search) => cx.command(search::ask(cx.env())),
            Some(Link::Reset) => {
                let env = *cx.env();
                cx.command(ask_reset(&env, None));
            }
            Some(Link::Restart) => {
                let question = cx.env().i18n().tr("settings.restart.question");
                cx.command(
                    paperpaw_sdk::ask::choice(question, &CHOSEN)
                        .about(RESTART)
                        .options("settings.option.", &["restart", "cancel"])
                        .command(),
                );
            }
            None => {}
        }
    }
}

impl Screen for GroupList {
    /// No pictures on it: whole-screen frames need no extra pass to count
    /// them (see [`Screen::pictures_in_grays`]).
    fn pictures_in_grays(&self) -> bool {
        false
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("settings.title"),
            None,
            paging(&self.rows),
        );
        let (entries, room) = (&self.entries, self.icon_room);
        self.rows
            .draw(&mut ui, self.keys, |ui, index, rect, focused| {
                let entry = &entries[index];
                let Some(link) = &entry.link else {
                    ui.heading(rect, &entry.title);
                    return;
                };
                if *link == Link::Search {
                    ui::SearchField::new(&entry.title)
                        .focused(focused)
                        .draw(ui, rect);
                    return;
                }
                let ink = style.focus_ink(focused);
                let icon = entry.icon.as_deref();
                let rect = ui.row_icon(rect, room, icon, ink);
                if *link == Link::Advanced {
                    let on = env.settings().read(&ADVANCED);
                    ui::Row::switch(&entry.title, on)
                        .focused(focused)
                        .draw(ui, rect);
                } else if matches!(link, Link::Reset | Link::Restart) {
                    ui::Row::value(&entry.title, "")
                        .focused(focused)
                        .draw(ui, rect);
                } else {
                    ui::Row::value(&entry.title, "›")
                        .focused(focused)
                        .draw(ui, rect);
                }
            });
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.covered.changed(cx) {
            // Hidden: laid out for the new size when it shows.
            return Handled::Yes;
        }
        let focus = self.rows.focus();
        let (style, header, rows, icon_room) = Self::fitted(cx.env(), &self.entries);
        (self.style, self.header, self.rows, self.icon_room) = (style, header, rows, icon_room);
        self.rows.set_focus(focus);
        cx.request_redraw();
        Handled::Yes
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if let Some(change) = navigate_rows(cx.env(), &mut self.rows, &mut self.keys, action) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.choose(self.rows.focus(), cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging(&self.rows));
        if let Some(action) = tapped {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.rows.hit(gesture.start) {
            Some(index) => {
                self.rows.set_focus(index);
                self.keys = ui::focus_shown(cx.env());
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = CHOSEN.read(event, params) {
            if picked.path == RESTART {
                if picked.option == "restart" {
                    cx.command(Command::Restart);
                }
            } else if picked.path.is_empty() {
                let env = *cx.env();
                reset(&env, &picked, cx);
            }
            return;
        }
        if let Some(typed) = search::ENTERED.read(event, params) {
            let text = typed.text.trim();
            if !text.is_empty() {
                cx.command(Command::OpenScreen(
                    search::SCREEN,
                    Params::new().with(params::TEXT, text.to_string()),
                ));
            }
            return;
        }
        if *event == events::SETTING_CHANGED && self.covered.changed(cx) {
            let env = *cx.env();
            self.rebuild(&env);
            cx.request_redraw();
        }
    }

    fn on_hide(&mut self) {
        self.covered.hide();
    }

    /// Asked for while covered: it comes once the page shows again, also
    /// below an overlay left open above it (where `on_show` does not).
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.on_show(cx);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.covered.shown() {
            let env = *cx.env();
            self.rebuild(&env);
            cx.request_redraw();
        }
    }
}
