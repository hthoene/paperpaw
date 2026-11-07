//! The pages of the settings, made from the registry: the main page, a
//! group's page, the chooser, themes and search. Each is built aside and
//! swapped into its view once whole.
use super::*;

/// Stages a new value for `key`.
pub(super) fn set_value(host: &Host<'_>, key: &str, value: &Val) -> Result<(), Error> {
    let mut scratch = buffer(TEXT_LIMIT + 256)?;
    let mut commands = CommandSender::new(host, &mut scratch);
    match value {
        Val::Bool(v) => commands.set_setting(key, SettingValue::Bool(*v)),
        Val::Int(v) => commands.set_setting(key, SettingValue::Int(*v)),
        Val::Text(v) => commands.set_setting(key, SettingValue::Text(v)),
    }
}

/// What decides which settings show, read now.
pub(super) fn context(host: &Host<'_>, scoped: bool) -> Result<Context, Error> {
    Ok(Context {
        advanced: native_data::read_bool(host, model::ADVANCED)?,
        developer: native_data::read_bool(host, model::DEVELOPER)?,
        scoped,
    })
}

/// Whether `setting` is listed: `offered` and, unless `advanced_ok`, shown at
/// the current level. Its number of choices is asked only when that decides.
pub(super) fn listed(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    cx: Context,
    setting: &Setting,
    advanced_ok: bool,
) -> Result<bool, Error> {
    let first = if advanced_ok {
        model::offered(groups, cx, setting, None)
    } else {
        model::visible(groups, cx, setting, None)
    };
    if !first {
        return Ok(false);
    }
    if setting.kind != Kind::Choice {
        return Ok(true);
    }
    let options = reader.detail(&setting.key)?.choices;
    Ok(options >= 2)
}

/// The group a setting is listed in.
pub(super) fn listed_in<'a>(groups: &[Group], setting: &'a Setting) -> &'a str {
    if groups.iter().any(|g| g.id == setting.group) {
        &setting.group
    } else {
        model::OTHER_GROUP
    }
}

/// The groups that hold a listed setting (`advanced_ok`: advanced ones too).
pub(super) fn holding(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    cx: Context,
    all: &[Setting],
    advanced_ok: bool,
) -> Result<Vec<String>, Error> {
    let mut found: Vec<String> = Vec::new();
    for setting in all {
        let group = listed_in(groups, setting);
        if !found.iter().any(|g| g == group) && listed(reader, groups, cx, setting, advanced_ok)? {
            push(&mut found, copy(group)?)?;
        }
    }
    Ok(found)
}

/// A catalog text; a key the catalog lacks is shown as it is.
pub(super) fn translate(
    messages: &mut Messages,
    host: &Host<'_>,
    key: &str,
) -> Result<String, Error> {
    match messages.copy(host, key) {
        Err(Error::NotFound | Error::Invalid) => copy(key),
        other => other,
    }
}

pub(super) fn on_off(messages: &mut Messages, host: &Host<'_>, on: bool) -> Result<String, Error> {
    messages.copy(host, if on { "core.on" } else { "core.off" })
}

/// The value of a setting as its row shows it.
pub(super) fn value_text(
    messages: &mut Messages,
    host: &Host<'_>,
    entry: &Entry,
    editing: Option<i64>,
) -> Result<String, Error> {
    let setting = &entry.setting;
    let text = match (&setting.value, editing) {
        (_, Some(now)) => model::with_unit(now, &entry.detail.unit)?,
        (Some(Val::Bool(on)), _) => on_off(messages, host, *on)?,
        (Some(Val::Int(v)), _) => model::with_unit(*v, &entry.detail.unit)?,
        (Some(Val::Text(v)), _) => match entry.options.iter().find(|(value, _)| value == v) {
            Some((_, name)) => copy(name)?,
            None => copy(v)?,
        },
        (None, _) => String::new(),
    };
    let text = if editing.is_some() {
        concat(&["‹ ", &text, " ›"])?
    } else if entry.detail.editor_kind != 0
        || (matches!(setting.kind, Kind::Choice | Kind::Language) && entry.options.len() != 2)
    {
        concat(&[&text, " ›"])?
    } else {
        text
    };
    // Kept for the book alone: marked.
    if setting.in_scope {
        concat(&["• ", &text])
    } else {
        Ok(text)
    }
}

impl GroupView {
    /// The setting row `index` shows.
    pub(super) fn entry(&self, index: usize) -> Option<&Entry> {
        match self.rows.get(index)? {
            Row::Setting(at) => self.entries.get(*at),
            _ => None,
        }
    }

    /// The item for row `index` of `rows`, with `editing` (row and value of a
    /// number being changed) shown.
    pub(super) fn item(
        &self,
        (rows, entries): (&[Row], &[Entry]),
        editing: Option<(usize, i64)>,
        messages: &mut Messages,
        host: &Host<'_>,
        index: usize,
    ) -> Result<Item, Error> {
        Ok(match &rows[index] {
            Row::Heading(title) => Item::heading(title)?,
            Row::Setting(at) => {
                let entry = entries.get(*at).ok_or(Error::State)?;
                let editing = editing.filter(|e| e.0 == index).map(|e| e.1);
                let value = value_text(messages, host, entry, editing)?;
                let mut item = Item::noted(&entry.setting.title, &value, &entry.detail.help)?;
                if self.touch
                    && entry.setting.kind == Kind::Int
                    && entry.detail.editor_kind == 0
                    && let Item::Row(row) = &mut item
                {
                    row.stepper = true;
                }
                item
            }
            Row::Reset => Item::row(&messages.copy(host, "settings.reset-page")?, "")?,
            Row::Theme => Item::row(&messages.copy(host, "settings.theme")?, "›")?,
            Row::Action(_, title, badge) => Item::row(title, badge)?,
            Row::Part(_, title) => Item::row(title, "›")?,
            Row::More => Item::row(&messages.copy(host, "settings.more")?, "›")?,
        })
    }

    /// The list's items for `rows` and `editing`, made off to the side: the
    /// view is not touched, so a refusal leaves what is shown as it was.
    pub(super) fn items_for(
        &self,
        host: &Host<'_>,
        (rows, entries): (&[Row], &[Entry]),
        editing: Option<(usize, i64)>,
    ) -> Result<Vec<Item>, Error> {
        let mut messages = Messages::new()?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for index in 0..rows.len() {
            items.push(self.item((rows, entries), editing, &mut messages, host, index)?);
        }
        Ok(items)
    }

    /// The editing state as the items show it.
    pub(super) fn shown_editing(&self) -> Option<(usize, i64)> {
        self.editing.as_ref().map(|e| (e.row, e.now))
    }

    /// Makes the list's items again from the rows (their values changed); on
    /// a refusal nothing changed.
    pub(super) fn rebuild_items(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let items = self.items_for(host, (&self.rows, &self.entries), self.shown_editing())?;
        self.list.replace(items);
        Ok(())
    }
}

/// Builds the main page.
pub(super) fn build_list(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut ListView,
) -> Result<(), Error> {
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let cx = context(host, false)?;
    let held = holding(reader, &groups, cx, &all, false)?;
    let widgets = reader.widget_ids()?;
    let pages = reader.menu(model::PAGES_MENU)?;
    let mut messages = Messages::new()?;
    // (order, headings before entries of the same order, link or heading, title)
    let mut placed: Vec<(i32, bool, Option<Link>, String)> = Vec::new();
    for PageRow {
        action,
        section,
        order,
        title,
        ..
    } in pages
    {
        let link = (!section).then_some(Link::Page(action));
        push(&mut placed, (order, !section, link, title))?;
    }
    for index in model::main_groups(&groups, &held, |id| widgets.iter().any(|w| w == id))? {
        let g = &groups[index];
        let order = if g.order == 0 {
            model::UNORDERED
        } else {
            g.order
        };
        let link = Link::Group {
            id: copy(&g.id)?,
            page: copy(&g.page)?,
        };
        push(&mut placed, (order, true, Some(link), copy(&g.title)?))?;
    }
    push(
        &mut placed,
        (
            i32::MIN,
            true,
            Some(Link::Search),
            messages.copy(host, "settings.search")?,
        ),
    )?;
    push(
        &mut placed,
        (
            i32::MAX,
            true,
            Some(Link::Advanced),
            messages.copy(host, "settings.advanced")?,
        ),
    )?;
    push(
        &mut placed,
        (
            i32::MAX,
            true,
            Some(Link::Reset),
            messages.copy(host, "settings.reset-all")?,
        ),
    )?;
    paperpaw_ui_kit::mem::stable_sort_by(&mut placed, |a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    // Headings with nothing under them go.
    let mut keep: Vec<bool> = Vec::new();
    keep.try_reserve_exact(placed.len())
        .map_err(|_| Error::NoMemory)?;
    for i in 0..placed.len() {
        keep.push(placed[i].2.is_some() || placed.get(i + 1).is_some_and(|next| next.2.is_some()));
    }
    let advanced = cx.advanced;
    let mut items = Vec::new();
    let mut links = Vec::new();
    for (i, (_, _, link, title)) in placed.into_iter().enumerate() {
        if !keep[i] {
            continue;
        }
        let item = match &link {
            None => Item::heading(&title)?,
            Some(Link::Advanced) => Item::row(&title, &on_off(&mut messages, host, advanced)?)?,
            Some(Link::Reset) => Item::row(&title, "")?,
            Some(_) => Item::row(&title, "›")?,
        };
        push(&mut items, item)?;
        push(&mut links, link)?;
    }
    // Made aside; the page changes once it is whole.
    view.links = links;
    view.list.replace(items);
    Ok(())
}

/// Builds a group's page.
pub(super) fn build_group(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut GroupView,
) -> Result<(), Error> {
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let cx = context(host, view.scoped)?;
    let mut messages = Messages::new()?;
    let of = if view.group.is_empty() {
        // A setting to focus, on its own group's page.
        match all.iter().find(|s| s.key == view.focus) {
            Some(s) => copy(listed_in(&groups, s))?,
            None => copy(model::OTHER_GROUP)?,
        }
    } else {
        copy(&view.group)?
    };
    let group = copy(&of)?;
    let title = match groups.iter().find(|g| g.id == of) {
        Some(g) => copy(&g.title)?,
        None => String::new(),
    };
    let title = if view.more {
        concat(&[&title, " · ", &messages.copy(host, "settings.more")?])?
    } else {
        title
    };
    // The page's settings: the basic ones, or the advanced ones alone.
    let mut specs: Vec<&Setting> = Vec::new();
    let mut advanced_offered = false;
    for s in all.iter().filter(|s| model::group_is(&groups, s, &of)) {
        if view.more {
            if s.has(flag::ADVANCED) && listed(reader, &groups, cx, s, true)? {
                push(&mut specs, s)?;
            }
        } else {
            if listed(reader, &groups, cx, s, false)? {
                push(&mut specs, s)?;
            }
            advanced_offered |= s.has(flag::ADVANCED) && model::offered(&groups, cx, s, None);
        }
    }
    // A page with nothing basic shows its advanced settings at once.
    if specs.is_empty() && !view.more {
        for s in all
            .iter()
            .filter(|s| model::group_is(&groups, s, &of) && s.has(flag::ADVANCED))
        {
            if listed(reader, &groups, cx, s, true)? {
                push(&mut specs, s)?;
            }
        }
    }
    let has_more = !view.more && advanced_offered && !specs.iter().any(|s| s.has(flag::ADVANCED));
    let mut pool: Vec<Option<Entry>> = Vec::new();
    for s in &specs {
        let detail = reader.detail(&s.key)?;
        let options = if matches!(s.kind, Kind::Choice | Kind::Language) {
            reader.choices(&s.key)?
        } else {
            Vec::new()
        };
        push(
            &mut pool,
            Some(Entry {
                setting: s.try_clone()?,
                detail,
                options,
            }),
        )?;
    }
    let (rows, entries) = page_rows(
        host,
        reader,
        view,
        (&groups, cx, &all, &of),
        pool,
        (has_more, !specs.is_empty()),
    )?;
    // Everything is made aside; the view changes once it is whole, so a
    // refusal leaves the rows and the items they belong to as they were.
    let items = view.items_for(host, (&rows, &entries), view.shown_editing())?;
    let focus = rows.iter().position(|r| match r {
        Row::Setting(at) => entries
            .get(*at)
            .is_some_and(|e| e.setting.key == view.focus),
        _ => false,
    });
    view.group = group;
    view.title = title;
    view.rows = rows;
    view.entries = entries;
    view.list.replace(items);
    if !view.focus.is_empty() {
        if let Some(at) = focus {
            view.list.set_focus(at);
        }
        view.focus = String::new();
    }
    Ok(())
}

/// The rows of a group's page from its settings: parts, actions, themes, the
/// settings by section, "More" and the reset.
pub(super) fn page_rows(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &GroupView,
    (groups, cx, all, of): (&[Group], Context, &[Setting], &str),
    mut pool: Vec<Option<Entry>>,
    (has_more, has_specs): (bool, bool),
) -> Result<(Vec<Row>, Vec<Entry>), Error> {
    let mut messages = Messages::new()?;
    let mut rows: Vec<Row> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    if !view.more {
        // Its parts, each a page of its own, where they hold something.
        let held = holding(reader, groups, cx, all, true)?;
        for g in groups.iter().filter(|g| g.parent == of) {
            if held.iter().any(|h| model::within(groups, h, &g.id)) {
                push(&mut rows, Row::Part(copy(&g.id)?, copy(&g.title)?))?;
            }
        }
        // What plugins do with the group.
        for page in reader.menu(of)? {
            if !page.section {
                push(&mut rows, Row::Action(page.action, page.title, copy("›")?))?;
            }
        }
        if of == LOOK_GROUP && !reader.presets(THEME_KIND)?.is_empty() {
            push(&mut rows, Row::Theme)?;
        }
    }
    let sections = model::section_order(pool.iter().flatten().map(|e| e.detail.section.as_str()))?;
    // Headings only where there is more than one part.
    let headed = sections.len() > 1;
    let mut owned: Vec<String> = Vec::new();
    for section in sections {
        push(&mut owned, copy(section)?)?;
    }
    let sections = owned;
    for section in &sections {
        if headed && !section.is_empty() {
            push(
                &mut rows,
                Row::Heading(translate(&mut messages, host, section)?),
            )?;
        }
        for slot in pool.iter_mut() {
            if slot.as_ref().is_some_and(|e| e.detail.section == *section)
                && let Some(entry) = slot.take()
            {
                push(&mut rows, Row::Setting(entries.len()))?;
                push(&mut entries, entry)?;
            }
        }
    }
    if has_more {
        push(&mut rows, Row::More)?;
    }
    if has_specs {
        push(&mut rows, Row::Reset)?;
    }
    Ok((rows, entries))
}

/// Reads one setting's value again into its row. `false` when the page does
/// not show it, or when its value decides what else shows (the page is made
/// anew then).
pub(super) fn refresh_row(host: &Host<'_>, view: &mut GroupView, key: &str) -> Result<bool, Error> {
    let Some(at) = view.rows.iter().position(|r| match r {
        Row::Setting(at) => view.entries.get(*at).is_some_and(|e| e.setting.key == key),
        _ => false,
    }) else {
        return Ok(false);
    };
    let Row::Setting(entry) = view.rows[at] else {
        return Ok(false);
    };
    if view.entries[entry].setting.has(flag::GOVERNS) {
        return Ok(false);
    }
    // The new value is shown first; it is stored once the items are made.
    let value = native_data::read(host, key)?;
    let old = core::mem::replace(&mut view.entries[entry].setting.value, value);
    match view.rebuild_items(host) {
        Ok(()) => Ok(true),
        Err(error) => {
            view.entries[entry].setting.value = old;
            Err(error)
        }
    }
}

/// Builds the chooser of a setting's value.
pub(super) fn build_choose(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut ChooseView,
) -> Result<(), Error> {
    let all = reader.settings("")?;
    let Some(setting) = all.iter().find(|s| s.key == view.key) else {
        return Ok(());
    };
    let title = copy(&setting.title)?;
    let options = reader.choices(&view.key)?;
    // More ways to choose: the entries of the menu named like the setting.
    let mut more = Vec::new();
    for page in reader.menu(&view.key)? {
        if !page.section {
            push(&mut more, (page.action, page.title))?;
        }
    }
    let current = native_data::read(host, &view.key)?;
    let mut items = Vec::new();
    for (value, name) in &options {
        let mark = if matches!(&current, Some(Val::Text(now)) if now == value) {
            MARK
        } else {
            ""
        };
        push(&mut items, Item::row(name, mark)?)?;
    }
    for (_, title) in &more {
        push(&mut items, Item::row(title, "›")?)?;
    }
    let at = options
        .iter()
        .position(|(value, _)| matches!(&current, Some(Val::Text(now)) if now == value))
        .unwrap_or(0);
    // Made aside; the page changes once it is whole.
    view.title = title;
    view.options = options;
    view.more = more;
    view.list.replace(items);
    view.list.set_focus(at);
    Ok(())
}

/// The appearance settings a theme covers.
pub(super) fn look_settings(reader: &mut Reader<'_, '_>) -> Result<Vec<String>, Error> {
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let mut keys = Vec::new();
    for s in &all {
        if model::within(&groups, listed_in(&groups, s), LOOK_GROUP)
            && !THEME_KEPT.contains(&s.key.as_str())
        {
            push(&mut keys, copy(&s.key)?)?;
        }
    }
    Ok(keys)
}

/// Builds the chooser of themes: none first, then the presets, the one in use marked.
pub(super) fn build_themes(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut ThemesView,
) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    let mut themes: Vec<(String, String)> = Vec::new();
    push(
        &mut themes,
        (String::new(), messages.copy(host, "settings.theme.none")?),
    )?;
    for (id, name) in reader.presets(THEME_KIND)? {
        push(&mut themes, (id, name))?;
    }
    let covered = look_settings(reader)?;
    let all = reader.settings("")?;
    let mut in_use: Option<usize> = None;
    // None: every covered setting has its default.
    if all
        .iter()
        .filter(|s| covered.contains(&s.key))
        .all(|s| s.is_default)
    {
        in_use = Some(0);
    }
    for (index, (id, _)) in themes.iter().enumerate().skip(1) {
        if in_use.is_some() {
            break;
        }
        let values = reader.preset_values(id)?;
        if model::applied(&all, &covered, &values) {
            in_use = Some(index);
        }
    }
    let mut items = Vec::new();
    for (index, (_, name)) in themes.iter().enumerate() {
        let mark = if in_use == Some(index) { MARK } else { "" };
        push(&mut items, Item::row(name, mark)?)?;
    }
    let at = in_use.unwrap_or(0);
    view.themes = themes;
    view.list.replace(items);
    view.list.set_focus(at);
    Ok(())
}

/// Finds the settings and pages whose names and descriptions hold the words.
pub(super) fn build_search(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut SearchView,
) -> Result<(), Error> {
    let words = model::words(&view.query)?;
    let mut hits: Vec<Target> = Vec::new();
    let mut items = Vec::new();
    if words.is_empty() {
        view.hits = hits;
        view.list.replace(items);
        return Ok(());
    }
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let cx = context(host, false)?;
    for s in &all {
        if !listed(reader, &groups, cx, s, false)? {
            continue;
        }
        let help = reader.detail(&s.key)?.help;
        if !model::matches(&concat(&[&s.title, " ", &help])?, &words)? {
            continue;
        }
        let group = listed_in(&groups, s);
        let note = groups
            .iter()
            .find(|g| g.id == group)
            .map_or("", |g| g.title.as_str());
        push(&mut items, Item::noted(&s.title, "›", note)?)?;
        push(
            &mut hits,
            Target::Setting {
                group: copy(group)?,
                key: copy(&s.key)?,
            },
        )?;
    }
    let mut messages = Messages::new()?;
    let settings_title = messages.copy(host, "settings.title")?;
    for page in reader.menu(model::PAGES_MENU)? {
        if !page.section && model::matches(&page.title, &words)? {
            push(&mut items, Item::noted(&page.title, "›", &settings_title)?)?;
            push(&mut hits, Target::Page(page.action))?;
        }
    }
    view.hits = hits;
    view.list.replace(items);
    Ok(())
}
