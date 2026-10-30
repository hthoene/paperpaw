//! A group's page: its settings with their values, changed in place or
//! in a chooser.

use super::*;
use paperpaw_sdk::ui::slider::{Slider, SliderDrag, SliderFocus, SliderView};

/// The id of setting `id` of group `of` for its placement `placement`:
/// `widgets.clock.face` of `widgets.clock@2` is `widgets.clock@2.face`.
/// Settings of the group named otherwise stay as they are.
pub(super) fn placed(id: &Id, of: &Id, placement: &Id) -> Id {
    id.as_str()
        .strip_prefix(of.as_str())
        .filter(|rest| rest.starts_with('.'))
        .and_then(|rest| Id::parse(&alloc::format!("{placement}{rest}")).ok())
        .unwrap_or_else(|| id.clone())
}

/// Puts the focus of rebuilt `rows` back at `focus`, or the nearest entry.
pub(super) fn refocus(rows: &mut ui::Rows, focus: usize) {
    if rows.is_empty() {
        return;
    }
    rows.set_focus(focus.min(rows.len() - 1));
    if rows.is_heading(rows.focus()) && rows.step(true) == ListChange::None {
        rows.step(false);
    }
}

// ---- Group page: settings ----------------------------------------------

/// The most of the screen a group's preview takes.
const PREVIEW_SHARE: u16 = 3;

/// What a group's settings do, shown above them (a widget that previews
/// the group, e.g. a sample of text for the reading settings), under a
/// heading.
pub(super) struct Preview {
    widget: Box<dyn Widget>,
    heading: Rect,
    area: Rect,
}

impl core::fmt::Debug for Preview {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Preview")
            .field("heading", &self.heading)
            .field("area", &self.area)
            .finish_non_exhaustive()
    }
}

impl Preview {
    /// The preview of `group`, if a widget previews it or the group it is
    /// a part of, at the top of `area`; `area` is left with the room below
    /// it.
    fn of(env: &Env<'_>, style: &UiStyle, group: &Id, area: &mut Rect) -> Option<Self> {
        let id = env.widgets().preview(group.as_str()).or_else(|| {
            let parent = env.settings().parent_of(group)?;
            env.widgets().preview(parent.as_str())
        })?;
        let widget = env.widgets().make(id.as_str(), env)?;
        let heading = ui::heading_height(env, style);
        let most = (env.device().display.height / PREVIEW_SHARE).saturating_sub(heading);
        let height = widget.height(env, area.width).min(most);
        if height == 0 {
            return None;
        }
        let heading = Rect::new(area.x, area.y, area.width, heading);
        let shown = Rect::new(area.x, heading.y + heading.height, area.width, height);
        // A line below it, then the settings.
        let used = heading.height + height + style.gap();
        *area = Rect::new(
            area.x,
            area.y + used,
            area.width,
            area.height.saturating_sub(used),
        );
        Some(Self {
            widget,
            heading,
            area: shown,
        })
    }

    fn draw(&self, ui: &mut ui::Ui<'_>) {
        let (env, style) = (ui.env(), ui.style());
        let all = self.heading.union(&self.area);
        if all.intersection(&ui.canvas().clip()).is_none() {
            return;
        }
        ui.heading(self.heading, env.i18n().tr("settings.preview"));
        self.widget.draw(ui.canvas(), env, self.area);
        // The line between rows, below it.
        let below = Rect::new(
            self.area.x,
            self.area.y,
            self.area.width,
            self.area.height + style.gap() / 2 + style.line(),
        );
        ui.separator(below, 0);
    }

    /// Lays it out anew for the values shown; `true` if it changed.
    fn update(&mut self, env: &Env<'_>) -> bool {
        self.widget.update(env)
    }
}

/// A row of a group page.
#[derive(Debug)]
pub(super) enum Row {
    /// A section's heading: its title.
    Heading(String),
    Setting(Box<SettingSpec>),
    /// Resets the page's settings to their defaults, at its end.
    Reset,
    /// An action plugins add to the group (a menu entry in the menu
    /// named by the group's id), after its settings: its title.
    Action(Id),
    /// The theme (or look of widgets) in use, at the top of the
    /// appearance settings where there are such presets (see
    /// [`themes`](super::themes)).
    Theme(super::themes::Chooser),
    /// A part of the group, a page of its own ([`SettingGroupSpec::within`]):
    /// its id and title.
    Part(Id, String),
    /// The group's advanced settings, a page one level down.
    More,
}

#[derive(Debug)]
pub(super) struct GroupPage {
    pub(super) group: Id,
    /// The group's advanced settings alone (its page one level down).
    pub(super) more: bool,
    pub(super) title: String,
    pub(super) entries: Vec<Row>,
    pub(super) style: UiStyle,
    pub(super) header: ui::Header,
    pub(super) rows: ui::Rows,
    /// The focus shows (buttons were used, or a number was tapped).
    pub(super) keys: bool,
    /// The focused number being changed with the buttons, as every
    /// screen that shows one does it (`ui::NumberEdit`).
    pub(super) edit: ui::NumberEdit,
    /// A slider's knob taken along by a finger: its row and the value it
    /// had before (shown, not kept, until the finger is lifted).
    pub(super) drag: SliderDrag,
    pub(super) dragged: Option<(usize, i64)>,
    /// Rows have room for descriptions.
    pub(super) noted: bool,
    /// The widest value of the page's numbers, so their − and + line up.
    pub(super) number_room: String,
    /// How each setting's row is laid out (none for sliders and others).
    lines: Vec<Option<ui::RowLines>>,
    /// What the settings do, shown above them, where the group has it.
    pub(super) preview: Option<Preview>,
    /// Covered by a full screen, and settings changed meanwhile.
    pub(super) covered: Covered,
}

/// The settings `group`'s page shows (`own`: the widget's group, for a
/// placement's): its basic ones, or its advanced ones alone (`more`).
fn specs(env: &Env<'_>, group: &Id, own: Option<&Id>, more: bool) -> Vec<SettingSpec> {
    let of = own.unwrap_or(group);
    let shown = |spec: &SettingSpec| match more {
        true => spec.level == Level::Advanced && offered(env, spec),
        false => visible(env, spec),
    };
    env.settings()
        .specs()
        .filter(|spec| env.settings().group_of(spec) == *of && shown(spec))
        .cloned()
        .map(|mut spec| {
            if own.is_some() {
                spec.id = placed(&spec.id, of, group);
            }
            spec
        })
        .collect()
}

/// The rows of group `of`'s page with `specs` (`placed`: a placement's,
/// `more`: its advanced settings alone): its parts, the themes, the
/// settings by section, its advanced ones a page down, what plugins do
/// with them and resetting them.
fn entries(env: &Env<'_>, of: &Id, placed: bool, more: bool, specs: &[SettingSpec]) -> Vec<Row> {
    // Advanced settings one level down, unless they are shown here (the
    // advanced switch, or a page of nothing else).
    let has_more = !more
        && !specs.iter().any(|spec| spec.level == Level::Advanced)
        && env.settings().specs().any(|spec| {
            env.settings().group_of(spec) == *of
                && spec.level == Level::Advanced
                && offered(env, spec)
        });
    // Its parts, each a page of its own, where they hold something (if
    // only advanced settings, shown there at once).
    let mut entries: Vec<Row> = match more || placed {
        true => Vec::new(),
        false => env
            .settings()
            .groups()
            .filter(|g| g.parent.as_ref() == Some(of))
            .filter(|g| {
                env.settings().specs().any(|spec| {
                    env.settings().within(&env.settings().group_of(spec), &g.id)
                        && offered(env, spec)
                })
            })
            .map(|g| Row::Part(g.id.clone(), env.i18n().tr(&g.title).to_string()))
            .collect(),
    };
    // What plugins do with the group (opening the update page, looking
    // for fonts): its first rows; after the settings where a preview shows
    // what they do, so that the two stay together.
    let actions: Vec<Row> = match !placed && !more {
        true => env
            .menus()
            .items(of.as_str())
            .into_iter()
            .filter(|item| !item.section && env.actions().contains(item.id.as_str()))
            .filter(|item| env.menus().state(env, item.id.as_str()).shown)
            .map(|item| Row::Action(item.id.clone()))
            .collect(),
        false => Vec::new(),
    };
    let previewed = env.widgets().preview(of.as_str()).is_some()
        || env
            .settings()
            .parent_of(of)
            .is_some_and(|parent| env.widgets().preview(parent.as_str()).is_some());
    let after = match previewed {
        true => actions,
        false => {
            entries.splice(0..0, actions);
            Vec::new()
        }
    };
    // Sections: settings without one first, then the standard ones,
    // then the others as they come.
    let mut sections: Vec<Option<Cow<'static, str>>> = Vec::new();
    for spec in specs {
        let section = spec.section_key();
        if !sections.contains(&section) {
            sections.push(section);
        }
    }
    let rank = |section: &Option<Cow<'static, str>>| match section {
        None => 0,
        Some(key) => group_sections::ORDER
            .iter()
            .position(|s| *s == key.as_ref())
            .map_or(group_sections::ORDER.len() + 1, |p| p + 1),
    };
    sections.sort_by_cached_key(rank);
    // Headings only where there is more than one part.
    let headed = sections.len() > 1;
    if *of == groups::LOOK && !placed && !more {
        use super::themes::{Chooser, themes};
        for chooser in [Chooser::Theme, Chooser::Widgets] {
            if themes(env, chooser).next().is_some() {
                entries.push(Row::Theme(chooser));
            }
        }
    }
    for section in &sections {
        if let Some(key) = section.as_deref().filter(|_| headed) {
            entries.push(Row::Heading(env.i18n().tr(key).to_string()));
        }
        entries.extend(
            specs
                .iter()
                .filter(|spec| spec.section_key() == *section)
                .cloned()
                .map(|spec| Row::Setting(Box::new(spec))),
        );
    }
    if has_more {
        entries.push(Row::More);
    }
    entries.extend(after);
    if !specs.is_empty() {
        entries.push(Row::Reset);
    }
    entries
}

/// An action's title and what it shows at the right: its state (`New`
/// for an update), else the arrow of a page.
fn action_texts<'a>(env: &'a Env<'_>, action: &Id) -> (&'a str, Cow<'static, str>) {
    let title = env.actions().get(action.as_str()).map_or("", |a| a.title());
    let value = match env.menus().state(env, action.as_str()).badge {
        Some(badge) => badge,
        None => Cow::Borrowed("›"),
    };
    (env.i18n().tr(title), value)
}

impl GroupPage {
    pub(super) fn new(env: &Env<'_>, group: Id) -> Self {
        Self::of(env, group, false)
    }

    /// The page of `group`, or of its advanced settings alone (`more`).
    pub(super) fn of(env: &Env<'_>, group: Id, more: bool) -> Self {
        // A placement's group (`widgets.clock@2`): the widget's settings,
        // as that placement has them.
        let own = group.unplaced();
        let of = own.as_ref().unwrap_or(&group);
        let title = env
            .settings()
            .groups()
            .find(|g| g.id == *of)
            .map_or_else(String::new, |g| env.i18n().tr(&g.title).to_string());
        let title = match more {
            true => alloc::format!("{title} · {}", env.i18n().tr("settings.more")),
            false => title,
        };
        // A page of advanced settings alone shows them, not a page of them
        // one level down.
        let basic = specs(env, &group, own.as_ref(), more);
        let specs = match basic.is_empty() && !more {
            true => specs(env, &group, own.as_ref(), true),
            false => basic,
        };
        let entries = entries(env, of, own.is_some(), more, &specs);
        let noted = specs.iter().any(|spec| spec.description.is_some());
        let (style, header, mut area) = page(env);
        // A placement's settings have none (they are the widget's).
        let preview = own
            .is_none()
            .then(|| Preview::of(env, &style, &group, &mut area))
            .flatten();
        let heading = ui::heading_height(env, &style);
        let row = if noted {
            style.noted_row()
        } else {
            u32::from(style.row())
        } as u16;
        let slider_row = ui::slider::row_height(&style, noted);
        let number_room = ui::number_room(
            env,
            &style,
            entries.iter().filter_map(|e| match e {
                Row::Setting(spec) => Some(&**spec),
                Row::Heading(_)
                | Row::Reset
                | Row::Theme(_)
                | Row::Action(_)
                | Row::Part(..)
                | Row::More => None,
            }),
        );
        // A title that does not fit beside its control gets a line of its
        // own, the control below it: titles are never cut. Decided here,
        // for every value a row may show, not each time it is drawn.
        let lines: Vec<Option<ui::RowLines>> = entries
            .iter()
            .map(|e| match e {
                Row::Setting(spec) if !ui::slider::shows(env, spec) => {
                    let (width, row) = (area.width, u32::from(row));
                    Some(ui::row_lines(
                        env,
                        &style,
                        spec,
                        width,
                        &number_room,
                        row,
                        noted,
                    ))
                }
                _ => None,
            })
            .collect();
        let heights = entries
            .iter()
            .zip(&lines)
            .map(|(e, lines)| match (e, lines) {
                (Row::Heading(_), _) => heading,
                (Row::Setting(_), Some(lines)) => lines.height,
                (Row::Setting(spec), None) => {
                    let title = env.i18n().tr(&spec.title);
                    slider_row
                        + ui::slider::title_extra(env, &style, area.width, title, &number_room)
                }
                (Row::Reset | Row::Theme(_) | Row::Action(_) | Row::Part(..) | Row::More, _) => {
                    style.row()
                }
            })
            .collect();
        let headings = entries
            .iter()
            .map(|e| matches!(e, Row::Heading(_)))
            .collect();
        Self {
            group,
            more,
            title,
            entries,
            style,
            header,
            rows: ui::Rows::new(area, heights, headings),
            keys: ui::focus_shown(env),
            edit: ui::NumberEdit::default(),
            drag: SliderDrag::default(),
            dragged: None,
            noted,
            number_room,
            lines,
            preview,
            covered: Covered::default(),
        }
    }

    /// Makes the page anew for the settings now, keeping its focus and a
    /// number being changed.
    fn rebuild(&mut self, env: &Env<'_>) {
        let focus = self.rows.focus();
        let (keys, drag, dragged) = (self.keys, self.drag, self.dragged);
        let edit = core::mem::take(&mut self.edit);
        *self = Self::of(env, self.group.clone(), self.more);
        refocus(&mut self.rows, focus);
        self.keys = keys;
        self.edit = edit;
        self.drag = drag;
        self.dragged = dragged;
    }

    /// Focuses setting `id`, shown so (e.g. found by a search).
    pub(super) fn focus_on(&mut self, _: &Env<'_>, id: &str) {
        let at = self
            .entries
            .iter()
            .position(|e| matches!(e, Row::Setting(spec) if spec.id.as_str() == id));
        if let Some(at) = at {
            self.rows.set_focus(at);
            self.keys = true;
        }
    }

    pub(super) fn spec(&self, index: usize) -> Option<&SettingSpec> {
        match self.entries.get(index) {
            Some(Row::Setting(spec)) => Some(spec),
            _ => None,
        }
    }

    pub(super) fn focused(&self) -> Option<&SettingSpec> {
        self.spec(self.rows.focus())
    }

    /// Changes a number by `step` at once (a tap on its − or +), or
    /// toggles a switch or a choice of two.
    pub(super) fn change(&self, env: &Env<'_>, step: i64, cx: &mut Cx<'_, '_>) {
        let Some(spec) = self.focused() else {
            return;
        };
        let current = env.settings().get(spec.id.as_str());
        let Some(value) = ui::next_value(env, spec, step) else {
            return;
        };
        if Some(&value) != current {
            cx.command(Command::SetSetting(spec.id.clone(), value));
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    /// Stops changing a number with the buttons without keeping what was
    /// shown, e.g. for a tap elsewhere.
    fn leave_editing(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        self.edit.leave(env, cx);
    }

    /// How much lower a slider's band is in `rect` for its long title
    /// ([`ui::slider::title_extra`]), as its row was laid out.
    fn slider_extra(&self, env: &Env<'_>, spec: &SettingSpec, rect: Rect) -> u16 {
        let title = env.i18n().tr(&spec.title);
        ui::slider::title_extra(env, &self.style, rect.width, title, &self.number_room)
    }

    /// The − and + of number row `index` in `rect`, where it shows them:
    /// a slider's in its band, else beside the value.
    fn steps(&self, env: &Env<'_>, index: usize, rect: Rect) -> Option<(Rect, Rect)> {
        let spec = self.spec(index)?;
        if ui::slider::shows(env, spec) {
            let extra = self.slider_extra(env, spec, rect);
            let band = ui::slider::band_below(&self.style, rect, extra);
            let parts = ui::slider::parts(&self.style, band);
            return Some((parts.minus, parts.plus));
        }
        let value = ui::setting_value(env, spec);
        // Its line, as high as − and + need.
        let line = match self.lines.get(index).copied().flatten() {
            Some(lines) => lines.at(rect).control,
            None => rect,
        };
        ui::value_stepper(env, &self.style, line, &self.number_room, &value)
    }

    /// Redraws row `index` where it is (a value shown that changed).
    fn redraw_row(&self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some(rect) = self.rows.rect_of(index) {
            cx.request_redraw_area(rect);
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    /// What confirming (or tapping) the focused setting does.
    pub(super) fn activate(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        match self.entries.get(self.rows.focus()) {
            Some(Row::Reset) => {
                cx.command(ask_reset(env, Some((&self.group, &self.title))));
                return;
            }
            Some(Row::Theme(chooser)) => {
                cx.command(Command::OpenScreen(super::themes::SCREEN, chooser.params()));
                return;
            }
            Some(Row::Action(action)) => {
                cx.command(Command::run(action.clone()));
                return;
            }
            Some(Row::Part(part, _)) => {
                let params = Params::new().with(params::ID, part.as_str().to_string());
                cx.command(Command::OpenScreen(GROUP_SCREEN, params));
                return;
            }
            Some(Row::More) => {
                let params = Params::new()
                    .with(params::ID, self.group.as_str().to_string())
                    .with(MORE, "1");
                cx.command(Command::OpenScreen(GROUP_SCREEN, params));
                return;
            }
            _ => {}
        }
        let Some(spec) = self.focused() else {
            return;
        };
        // Its editor, the keyboard, the list of its values.
        if let Some(command) = ui::setting_opens(env, spec) {
            cx.command(command);
            return;
        }
        match spec.kind {
            SettingKind::Int { .. } => {
                let spec = spec.clone();
                self.edit.start(env, &spec, cx);
            }
            _ => self.change(env, 1, cx),
        }
    }
}

impl Screen for GroupPage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, &self.title, None, paging(&self.rows));
        if let Some(preview) = &self.preview {
            preview.draw(&mut ui);
        }
        let entries = &self.entries;
        let editing = self.edit.active();
        let noted = self.noted;
        let room = self.number_room.as_str();
        let all_lines = &self.lines;
        let touch = env.device().input.touch;
        self.rows
            .draw(&mut ui, self.keys, |ui, index, rect, focused| {
                // The rows that are not settings: a row high.
                let row = Rect::new(rect.x, rect.y, rect.width, style.row());
                let spec = match &entries[index] {
                    Row::Heading(title) => return ui.heading(rect, title),
                    Row::Reset => {
                        let title = env.i18n().tr("settings.reset-page");
                        return ui::Row::plain(title).focused(focused).draw(ui, row);
                    }
                    Row::Theme(chooser) => {
                        let title = env.i18n().tr(chooser.title());
                        let name = super::themes::applied_name(env, *chooser);
                        return ui::Row::value(title, &name).focused(focused).draw(ui, row);
                    }
                    Row::Action(action) => {
                        let (title, value) = action_texts(env, action);
                        return ui::Row::value(title, &value).focused(focused).draw(ui, row);
                    }
                    Row::Part(_, title) => {
                        return ui::Row::value(title, "›").focused(focused).draw(ui, row);
                    }
                    Row::More => {
                        let title = env.i18n().tr("settings.more");
                        return ui::Row::value(title, "›").focused(focused).draw(ui, row);
                    }
                    Row::Setting(spec) => spec,
                };
                let canvas = ui.canvas();
                let title = env.i18n().tr(&spec.title);
                let note = spec.description.as_deref().map_or("", |d| env.i18n().tr(d));
                let note = if noted { note } else { "" };
                let mut value = ui::setting_value(env, spec);
                if let Some(slider) = Slider::of(spec).filter(|_| ui::slider::shows(env, spec)) {
                    let focus = match (focused, editing) {
                        (false, _) => SliderFocus::None,
                        (true, false) => SliderFocus::Focused,
                        (true, true) => SliderFocus::Editing,
                    };
                    let view = SliderView::new(rect, env.settings().int(spec.id.as_str()), &value)
                        .title(title)
                        .note(note)
                        .focus(focus)
                        .room(&self.number_room);
                    slider.draw_row(canvas, env, &style, &view);
                    return;
                }
                let lines = match all_lines.get(index).copied().flatten() {
                    Some(lines) => lines.at(rect),
                    None => ui::RowLines::one(rect),
                };
                match spec.kind {
                    SettingKind::Int { .. } if focused => {
                        ui::draw_stepper_lines(
                            canvas, env, &style, lines, title, &value, room, editing,
                        );
                        if !note.is_empty() {
                            ui::draw_lines_note(canvas, env, &style, rect, lines, true, note);
                        }
                        return;
                    }
                    // On touch screens − and + are always there.
                    SettingKind::Int { .. } if touch => {
                        ui::draw_number_lines(
                            canvas, env, &style, lines, title, &value, room, None,
                        );
                        if !note.is_empty() {
                            ui::draw_lines_note(canvas, env, &style, rect, lines, false, note);
                        }
                        return;
                    }
                    SettingKind::Bool if spec.extra().editor.is_none() => {
                        let on = env.settings().bool(spec.id.as_str());
                        ui::draw_switch_lines(
                            canvas, env, &style, rect, lines, focused, title, note, on,
                        );
                        return;
                    }
                    _ => {}
                }
                if ui::opens_screen(env, spec) {
                    value.push_str(" ›");
                }
                ui::draw_setting_row(
                    canvas, env, &style, rect, lines, focused, title, &value, note,
                );
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        if self.edit.active() {
            let spec = self.focused().cloned();
            return self
                .edit
                .on_action(&env, spec.as_ref(), action, cx)
                .handled();
        }
        if let Some(change) = navigate_rows(&mut self.rows, &mut self.keys, action) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.activate(&env, cx);
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
        let Some(index) = self.rows.hit(gesture.start) else {
            return Handled::No;
        };
        let env = *cx.env();
        let is_number = matches!(
            self.spec(index).map(|s| &s.kind),
            Some(SettingKind::Int { .. })
        );
        if is_number && env.device().input.touch {
            // − and + are always there: a tap on one changes the number,
            // elsewhere on the row nothing happens.
            self.rows.set_focus(index);
            self.keys = false;
            self.leave_editing(&env, cx);
            let Some(rect) = self.rows.rect_of(index) else {
                return Handled::Yes;
            };
            if let Some((minus, plus)) = self.steps(&env, index, rect) {
                let x = gesture.start.x;
                // A little room around the buttons: easier to hit.
                let slack = i32::from(self.style.gap());
                let within = |r: Rect| x >= i32::from(r.x) - slack && x < r.right() as i32 + slack;
                if within(minus) {
                    self.change(&env, -1, cx);
                } else if within(plus) {
                    self.change(&env, 1, cx);
                }
            }
            return Handled::Yes;
        }
        if is_number {
            // The first tap selects a number; then its − and + buttons
            // change it, so a value never changes by accident.
            if index != self.rows.focus() || !self.keys {
                self.rows.set_focus(index);
                self.keys = true;
                self.leave_editing(&env, cx);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
                return Handled::Yes;
            }
            let Some(rect) = self.rows.rect_of(index) else {
                return Handled::Yes;
            };
            if let Some((minus, plus)) = self.steps(&env, index, rect) {
                let x = gesture.start.x;
                let within = |r: Rect| x >= i32::from(r.x) && x < r.right() as i32;
                if within(minus) {
                    self.change(&env, -1, cx);
                } else if within(plus) {
                    self.change(&env, 1, cx);
                }
            }
            return Handled::Yes;
        }
        // Anything else acts at once; the focus hides again.
        let keys = ui::focus_shown(&env);
        if index != self.rows.focus() || keys != self.keys {
            self.rows.set_focus(index);
            self.keys = keys;
            self.leave_editing(&env, cx);
            cx.request_redraw();
        }
        self.activate(&env, cx);
        Handled::Yes
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        if !env.device().input.touch {
            return Handled::No;
        }
        // The row of the knob taken, or of a finger coming down.
        let index = match self.dragged {
            Some((index, _)) => Some(index),
            None if event.phase == PointerPhase::Down => self.rows.hit(event.point),
            None => None,
        };
        let Some((index, slider, id, rect)) = index.and_then(|i| {
            let spec = self.spec(i).filter(|s| ui::slider::shows(&env, s))?;
            let slider = Slider::of(spec)?;
            Some((i, slider, spec.id.clone(), self.rows.rect_of(i)?))
        }) else {
            return Handled::No;
        };
        let extra = self
            .spec(index)
            .map_or(0, |spec| self.slider_extra(&env, spec, rect));
        let band = ui::slider::band_below(&self.style, rect, extra);
        let value = env.settings().int(id.as_str());
        let Some(step) = self
            .drag
            .on_pointer(event, &self.style, &slider, band, value)
        else {
            return Handled::No;
        };
        let before = match self.dragged {
            Some((_, before)) => before,
            None => {
                let before = env.settings().int(id.as_str());
                self.dragged = Some((index, before));
                // The focus hides, as after other taps.
                self.rows.set_focus(index);
                self.keys = false;
                self.leave_editing(&env, cx);
                cx.request_redraw();
                before
            }
        };
        // Only the sizes of book text that look different.
        let shown = |value: i64| {
            self.spec(index)
                .map_or(value, |s| ui::shown_slider_value(&env, s, before, value))
        };
        match step {
            ui::slider::SliderStep::Moved(value) => {
                let value = shown(value);
                cx.command(Command::PreviewSettings(alloc::vec![(
                    id,
                    Some(SettingValue::Int(value))
                )]));
            }
            ui::slider::SliderStep::Same => {}
            ui::slider::SliderStep::Chosen(value) => {
                let value = shown(value);
                self.dragged = None;
                if value != before {
                    cx.command(Command::SetSetting(id, SettingValue::Int(value)));
                }
                end_preview(&env, cx);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            ui::slider::SliderStep::Cancelled => {
                self.dragged = None;
                end_preview(&env, cx);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            _ => {}
        }
        Handled::Yes
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        // While the buttons change a number: what each does to it.
        self.edit.hint(action, env)
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

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = CHOSEN.read(event, params) {
            if picked.path == self.group.as_str() {
                let env = *cx.env();
                reset(&env, &picked, cx);
            }
            return;
        }
        if *event == events::SETTING_CHANGED {
            let covered = self.covered.changed(cx);
            let env = *cx.env();
            // A setting shown only while this one has a value.
            let shows_others = events::SETTING_CHANGED
                .read(event, params)
                .is_some_and(|changed| env.settings().governs(changed.id.as_str()));
            if covered || shows_others {
                self.rebuild(&env);
                cx.request_redraw();
            }
        } else if *event == events::PREVIEW_CHANGED {
            // A value being chosen, shown: its row and what it does.
            let env = *cx.env();
            let index = self.dragged.map_or(self.rows.focus(), |(index, _)| index);
            self.redraw_row(index, cx);
            if let Some(preview) = self.preview.as_mut()
                && preview.update(&env)
            {
                cx.request_redraw_area(preview.area);
            }
        }
    }
}
