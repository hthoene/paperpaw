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

/// The groups that hold a listed setting (`advanced_ok`: advanced ones too),
/// found a page of the registry at a time: no list of settings is kept.
pub(super) fn holding(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    cx: Context,
    advanced_ok: bool,
) -> Result<Vec<String>, Error> {
    let mut found: Vec<String> = Vec::new();
    reader.each_setting(
        "",
        Fields::Light,
        |_| true,
        |reader, setting| {
            let group = listed_in(groups, &setting);
            if !found.iter().any(|g| g == group)
                && listed(reader, groups, cx, &setting, advanced_ok)?
            {
                push(&mut found, copy(group)?)?;
            }
            Ok(())
        },
    )?;
    Ok(found)
}

/// The registry scope that asks for the settings shown in group `of` and no
/// others (op 1: a group's own settings, not its parts'); empty where `of` is
/// no group, so that nothing is taken for a plugin's name.
pub(super) fn scope_of<'a>(groups: &[Group], of: &'a str) -> &'a str {
    if of == model::OTHER_GROUP || groups.iter().any(|g| g.id == of) {
        of
    } else {
        ""
    }
}

/// Whether group `part` or any part of it holds a listed setting: asked of the
/// registry one group at a time, and no more once one is found, where the
/// whole list was walked for it.
pub(super) fn holds(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    cx: Context,
    part: &str,
) -> Result<bool, Error> {
    let mut found = false;
    for g in groups.iter().filter(|g| model::within(groups, &g.id, part)) {
        reader.each_setting(
            &g.id,
            Fields::Light,
            |_| true,
            |reader, setting| {
                if !found && listed(reader, groups, cx, &setting, true)? {
                    found = true;
                }
                Ok(())
            },
        )?;
        if found {
            break;
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

/// A list keeps ids in storage and shows their names in order: those the
/// registry gives its items (`names`, as Beta 3 showed status items and
/// widgets), else their catalogue names.
fn list_value_text(
    messages: &mut Messages,
    host: &Host<'_>,
    value: &str,
    names: &[(String, String)],
) -> Result<String, Error> {
    let mut text = String::new();
    for id in native_data::list_ids(value) {
        let name = match names.iter().find(|(of, _)| of == native_data::base_id(id)) {
            Some((_, name)) => copy(name)?,
            None => translate(messages, host, id)?,
        };
        text.try_reserve(name.len().saturating_add(2))
            .map_err(|_| Error::NoMemory)?;
        if !text.is_empty() {
            text.push_str(", ");
        }
        text.push_str(&name);
    }
    if text.is_empty() {
        messages.copy(host, "core.list.none")
    } else {
        Ok(text)
    }
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
        (_, Some(now)) => messages.with_unit(host, now, &entry.detail.unit)?,
        (Some(Val::Bool(on)), _) => on_off(messages, host, *on)?,
        (Some(Val::Int(v)), _) => messages.with_unit(host, *v, &entry.detail.unit)?,
        (Some(Val::Text(v)), _) if setting.kind == Kind::List => {
            list_value_text(messages, host, v, &entry.options)?
        }
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
                // A long value goes to a line of its own, as in Beta 3.
                let mut item = Item::noted(&entry.setting.title, &value, &entry.detail.help)?
                    .with_wrapped_value();
                // A switch is drawn as one, with its state told as the item's.
                if let (Kind::Bool, Some(Val::Bool(on)), Item::Row(row)) =
                    (entry.setting.kind, &entry.setting.value, &mut item)
                {
                    row.on = Some(*on);
                }
                if entry.setting.kind == Kind::Int && entry.detail.editor_kind == 0 {
                    match self.track_of(entry, editing) {
                        // Without touch, Left and Right change it: its − and
                        // + show with the focus, as in Beta 3.
                        _ if !self.touch => item = item.with_focus_steps()?,
                        Some(track) => item = item.with_track(track)?,
                        None => {
                            if let Item::Row(row) = &mut item {
                                row.stepper = true;
                            }
                        }
                    }
                }
                item
            }
            Row::Reset => Item::row(&messages.copy(host, "settings.reset-page")?, "")?,
            Row::Look(look) => Item::row(&messages.copy(host, look.title())?, "›")?,
            Row::Action(_, title, badge) => Item::row(title, badge)?,
            Row::Part(_, title) => Item::row(title, "›")?,
            Row::More => Item::row(&messages.copy(host, "settings.more")?, "›")?,
        })
    }

    /// The slider a number row shows (touch screens, numbers that ask for one),
    /// at `editing` where it is being changed.
    fn track_of(&self, entry: &Entry, editing: Option<i64>) -> Option<Track> {
        let (min, max) = (entry.detail.min, entry.detail.max);
        let Some(Val::Int(now)) = entry.setting.value else {
            return None;
        };
        (entry.setting.has(flag::SLIDER) && max > min).then(|| Track {
            value: editing.unwrap_or(now).clamp(min, max),
            range: (min, max),
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

/// A row of the main page before it is sorted: its order, whether it is listed
/// before the entries of the same order, what it opens (none: a heading), and
/// its title and icon.
type Placed = (i32, bool, Option<Link>, (String, String));

/// The items of the main page for the rows `placed` (sorted), with what each
/// opens: headings with nothing under them go.
fn main_items(
    host: &Host<'_>,
    placed: Vec<Placed>,
    advanced: bool,
) -> Result<(Vec<Item>, Vec<Option<Link>>), Error> {
    let mut keep: Vec<bool> = Vec::new();
    keep.try_reserve_exact(placed.len())
        .map_err(|_| Error::NoMemory)?;
    for i in 0..placed.len() {
        keep.push(placed[i].2.is_some() || placed.get(i + 1).is_some_and(|next| next.2.is_some()));
    }
    let mut items = Vec::new();
    let mut links = Vec::new();
    for (i, (_, _, link, (title, icon))) in placed.into_iter().enumerate() {
        if !keep[i] {
            continue;
        }
        let item = match &link {
            None => Item::heading(&title)?,
            // A switch, as it was in Beta 3.
            Some(Link::Advanced) => {
                let mut item = Item::row(&title, "")?;
                if let Item::Row(row) = &mut item {
                    row.on = Some(advanced);
                }
                item
            }
            // The search looks like the field it opens, as it did in Beta 3.
            Some(Link::Search) => Item::field(&title)?,
            Some(Link::Reset | Link::Restart) => Item::row(&title, "")?,
            Some(_) => Item::row(&title, "›")?,
        };
        let item = with_icon(host, item, &icon)?;
        push(&mut items, item)?;
        push(&mut links, link)?;
    }
    Ok((items, links))
}

/// Builds the main page.
pub(super) fn build_list(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut ListView,
) -> Result<(), Error> {
    // The secrets are asked first, through the reader's buffers: the service
    // holds its own while it answers, and the group list is not built yet.
    let pages = reader.menu(model::PAGES_MENU)?;
    let secrets_kept = crate::native_secrets::shown(host, &pages, reader.buffers())?;
    let groups = reader.groups()?;
    let cx = context(host, false)?;
    let held = holding(reader, &groups, cx, false)?;
    let widgets = reader.widget_ids()?;
    let mut messages = Messages::new()?;
    // (order, headings before entries of the same order, link or heading, title)
    let mut placed: Vec<Placed> = Vec::new();
    for PageRow {
        action,
        section,
        order,
        title,
        icon,
        ..
    } in pages
    {
        // Passwords and keys show once there are some.
        if action == crate::native_secrets::OPEN && !secrets_kept {
            continue;
        }
        let link = (!section).then_some(Link::Page(action));
        push(&mut placed, (order, !section, link, (title, icon)))?;
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
        push(
            &mut placed,
            (order, true, Some(link), (copy(&g.title)?, copy(&g.icon)?)),
        )?;
    }
    push(
        &mut placed,
        (
            i32::MIN,
            true,
            Some(Link::Search),
            (messages.copy(host, "settings.search")?, copy("search")?),
        ),
    )?;
    push(
        &mut placed,
        (
            i32::MAX,
            true,
            Some(Link::Advanced),
            (messages.copy(host, "settings.advanced")?, copy("eye")?),
        ),
    )?;
    push(
        &mut placed,
        (
            i32::MAX,
            true,
            Some(Link::Reset),
            (messages.copy(host, "settings.reset-all")?, copy("refresh")?),
        ),
    )?;
    // Last: what plugins saved goes along, as when the device goes off.
    push(
        &mut placed,
        (
            i32::MAX,
            true,
            Some(Link::Restart),
            (messages.copy(host, "settings.restart")?, copy("power")?),
        ),
    )?;
    paperpaw_ui_kit::mem::stable_sort_by(&mut placed, |a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    let (items, links) = main_items(host, placed, cx.advanced)?;
    // Made aside; the page changes once it is whole.
    view.links = links;
    view.list.replace(items);
    Ok(())
}

/// `item` with the icon named `name` at the start of its row, where the host
/// has an icon font with that icon; a heading or a name that is empty is left
/// as it is.
pub(super) fn with_icon(host: &Host<'_>, mut item: Item, name: &str) -> Result<Item, Error> {
    if !name.is_empty()
        && let Item::Row(row) = &mut item
    {
        row.icon = paperpaw_ui_kit::icons::glyph(host, name)?;
    }
    Ok(item)
}

/// The entry of a page's setting. On a placement's page (`of`, the placement
/// and the keys it sets itself) the entry has the placement's key and value;
/// a value the placement does not set counts as the default.
fn entry_of(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    s: &Setting,
    placement: Option<(&str, &str, &[String])>,
) -> Result<Entry, Error> {
    let detail = reader.detail(&s.key)?;
    let mut setting = s.try_clone()?;
    if let Some((of, placed, set)) = placement
        && let Some(key) = model::placed(&s.key, of, placed)?
    {
        setting.value = native_data::read(host, &key)?;
        setting.is_default = !set.contains(&key);
        setting.in_scope = false;
        setting.key = key;
    } else if setting.value.is_none()
        && matches!(setting.kind, Kind::Text | Kind::List)
        && !setting.has(flag::SECRET)
    {
        // The registry's row leaves out a value too long for it: read whole.
        setting.value = native_data::read(host, &setting.key)?;
    }
    let options = match (setting.kind, &setting.value) {
        (Kind::Choice | Kind::Language, _) => reader.choices(&s.key)?,
        // A list shows the names of its items, as the registry has them.
        (Kind::List, Some(Val::Text(ids))) => reader.item_names(ids)?,
        _ => Vec::new(),
    };
    Ok(Entry {
        setting,
        detail,
        options,
    })
}

/// The group the setting `focus` is listed in (the other group where it is not
/// known).
fn focus_group(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    focus: &str,
) -> Result<String, Error> {
    match reader.setting_row(focus, Fields::Light)? {
        Some(s) => copy(listed_in(groups, &s)),
        None => copy(model::OTHER_GROUP),
    }
}

/// Builds a group's page.
pub(super) fn build_group(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut GroupView,
) -> Result<(), Error> {
    let groups = reader.groups()?;
    let cx = context(host, view.scoped)?;
    let mut messages = Messages::new()?;
    // A placement's page (`widgets.clock@2`) shows the settings of its widget's
    // group, as that placement has them.
    let placement = model::unplaced(&view.group);
    let of = if let Some(base) = placement {
        copy(base)?
    } else if view.group.is_empty() {
        // A setting to focus, on its own group's page.
        focus_group(reader, &groups, &view.focus)?
    } else {
        copy(&view.group)?
    };
    let group = if placement.is_some() {
        copy(&view.group)?
    } else {
        copy(&of)?
    };
    // What the placement sets itself; the rest shows the widget's own value.
    let set = if placement.is_some() {
        reader.placement_keys(&view.group)?
    } else {
        Vec::new()
    };
    let title = page_title(host, &groups, &of, view.more, &mut messages)?;
    // The settings of the page's group, and only those are read.
    let mut mine: Vec<Setting> = Vec::new();
    reader.each_setting(
        scope_of(&groups, &of),
        Fields::Full,
        |row| model::in_group(&groups, row.group(), &of),
        |_, s| push(&mut mine, s),
    )?;
    // The page's settings: the basic ones, or the advanced ones alone.
    let mut specs: Vec<&Setting> = Vec::new();
    let mut advanced_offered = false;
    for s in &mine {
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
        for s in mine.iter().filter(|s| s.has(flag::ADVANCED)) {
            if listed(reader, &groups, cx, s, true)? {
                push(&mut specs, s)?;
            }
        }
    }
    font_first(&mut specs, &of);
    let has_more = !view.more && advanced_offered && !specs.iter().any(|s| s.has(flag::ADVANCED));
    let mut pool: Vec<Option<Entry>> = Vec::new();
    for s in &specs {
        let of_placement = placement.map(|_| (of.as_str(), view.group.as_str(), set.as_slice()));
        push(&mut pool, Some(entry_of(host, reader, s, of_placement)?))?;
    }
    let (rows, entries) = page_rows(
        host,
        reader,
        view,
        (&groups, cx, &of),
        pool,
        (has_more, !specs.is_empty()),
    )?;
    // What the settings do, above them (a sample of text for the reading
    // settings): from a widget that previews the group or the group it is a part
    // of; its room is the list's (`preview.rs`).
    let preview = if view.more || placement.is_some() {
        None
    } else {
        previewing(reader, &groups, &of)?
    };
    // Everything is made aside; the view changes once it is whole, so a
    // refusal leaves the rows and the items they belong to as they were.
    // A number being changed stays on its setting's row, wherever it is now.
    let editing = view.editing.as_ref().and_then(|e| {
        let row = rows.iter().position(|r| {
            matches!(r, Row::Setting(at) if entries.get(*at).is_some_and(|en| en.setting.key == e.key))
        })?;
        Some((row, e.now))
    });
    let items = view.items_for(host, (&rows, &entries), editing)?;
    let focus = rows.iter().position(|r| match r {
        Row::Setting(at) => entries
            .get(*at)
            .is_some_and(|e| e.setting.key == view.focus),
        _ => false,
    });
    let first_setting = (view.rows.is_empty())
        .then(|| first_real_setting(&group, &rows))
        .flatten();
    view.group = group;
    view.title = title;
    view.preview = preview;
    view.rows = rows;
    view.entries = entries;
    view.list.replace(items);
    match (editing, view.editing.as_mut()) {
        (Some((row, _)), Some(kept)) => kept.row = row,
        // Its setting is no longer on the page: the value kept stays.
        (None, Some(_)) => {
            view.editing = None;
            CommandSender::new(host, &mut [0; 64]).end_preview()?;
            super::edit_hints(host, false);
        }
        _ => {}
    }
    if view.preview.is_some() {
        view.list
            .set_head(&messages.copy(host, "settings.preview")?)?;
    }
    if !view.focus.is_empty() {
        if let Some(at) = focus {
            view.list.set_focus(at);
        }
        view.focus = String::new();
    } else if let Some(at) = first_setting {
        view.list.set_focus(at);
    }
    Ok(())
}

/// A widget's font is what its page is opened for as often as not: first,
/// not after the widget's own settings (a page or two of buttons away).
fn font_first(specs: &mut [&Setting], of: &str) {
    if let Some(at) = specs.iter().position(|s| is_font_of(of, &s.key)) {
        specs[..=at].rotate_right(1);
    }
}

/// Whether `key` is the font setting of widget `of` (`<widget>.font`).
fn is_font_of(of: &str, key: &str) -> bool {
    key.strip_prefix(of).is_some_and(|rest| rest == ".font")
}

/// The title of the page of group `of`, with "More" after it for its
/// advanced settings.
fn page_title(
    host: &Host<'_>,
    groups: &[Group],
    of: &str,
    more: bool,
    messages: &mut Messages,
) -> Result<String, Error> {
    let title = match groups.iter().find(|g| g.id == of) {
        Some(g) => copy(&g.title)?,
        None => String::new(),
    };
    if more {
        concat(&[&title, " · ", &messages.copy(host, "settings.more")?])
    } else {
        Ok(title)
    }
}

/// The widget that previews `group`, or the group it is a part of. A registry
/// that cannot say leaves the page without one; only a full heap is an error.
fn previewing(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    group: &str,
) -> Result<Option<String>, Error> {
    let parent = groups
        .iter()
        .find(|g| g.id == group)
        .map(|g| g.parent.as_str())
        .filter(|parent| !parent.is_empty());
    for id in core::iter::once(group).chain(parent) {
        match reader.previewing(id) {
            Ok(Some(widget)) => return Ok(Some(widget)),
            Ok(None) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        }
    }
    Ok(None)
}

/// The row a page opens its focus on when an action another plugin added
/// stands above its settings (as Discover fonts): its first setting. A group's
/// own action (Mail's *Open mail*) is the page's first row, so none then.
fn first_real_setting(group: &str, rows: &[Row]) -> Option<usize> {
    let own = group.split('.').next().unwrap_or_default();
    let foreign_first = matches!(
        rows.first(),
        Some(Row::Action(action, ..)) if action.split('.').next().unwrap_or_default() != own
    );
    if foreign_first {
        rows.iter().position(|r| matches!(r, Row::Setting(_)))
    } else {
        None
    }
}

/// The rows of a group's page from its settings: parts, actions, themes, the
/// settings by section, "More" and the reset.
pub(super) fn page_rows(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &GroupView,
    (groups, cx, of): (&[Group], Context, &str),
    mut pool: Vec<Option<Entry>>,
    (has_more, has_specs): (bool, bool),
) -> Result<(Vec<Row>, Vec<Entry>), Error> {
    let mut messages = Messages::new()?;
    let mut rows: Vec<Row> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    // A placement's page has no parts, actions or themes: those are the group's.
    if !view.more && model::unplaced(&view.group).is_none() {
        // Its parts, each a page of its own, where they hold something.
        for g in groups.iter().filter(|g| g.parent == of) {
            if holds(reader, groups, cx, &g.id)? {
                push(&mut rows, Row::Part(copy(&g.id)?, copy(&g.title)?))?;
            }
        }
        // What plugins do with the group.
        for page in reader.menu(of)? {
            // At the right the state its item shows (a badge such as
            // *New*), else the way on.
            if !page.section {
                let right = if page.badge.is_empty() {
                    copy("›")?
                } else {
                    page.badge
                };
                push(&mut rows, Row::Action(page.action, page.title, right))?;
            }
        }
        if of == LOOK_GROUP {
            for look in [Look::Theme, Look::Widgets] {
                if !reader.presets(look.kind())?.is_empty() {
                    push(&mut rows, Row::Look(look))?;
                }
            }
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
    // With it whether it is the default and the book's own, which the mark
    // of a value for this book only shows.
    let value = native_data::read(host, key)?;
    let mut reader = Reader::open(host)?;
    let found = reader.setting_row(key, Fields::Light);
    let source = reader.finish(found)?;
    let setting = &mut view.entries[entry].setting;
    let old = (setting.value.take(), setting.is_default, setting.in_scope);
    setting.value = value;
    if let Some(source) = source {
        (setting.is_default, setting.in_scope) = (source.is_default, source.in_scope);
    }
    match view.rebuild_items(host) {
        Ok(()) => Ok(true),
        Err(error) => {
            let setting = &mut view.entries[entry].setting;
            (setting.value, setting.is_default, setting.in_scope) = old;
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
    // A placement's setting is chosen among the options of the setting it
    // stands for; its value is its own.
    let base = match model::unplaced_key(&view.key)? {
        Some(base) => base,
        None => copy(&view.key)?,
    };
    let Some(title) = reader.setting_row(&base, Fields::Full)?.map(|s| s.title) else {
        return Ok(());
    };
    let options = reader.choices(&base)?;
    // More ways to choose: the entries of the menu named like the setting.
    let mut more = Vec::new();
    for page in reader.menu(&base)? {
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

/// The settings the presets of `look` cover: the appearance settings a theme
/// gives values (what the reader needs to read at all is kept), or the
/// settings of every widget's frame, room, sizes and colours.
pub(super) fn look_settings(reader: &mut Reader<'_, '_>, look: Look) -> Result<Vec<String>, Error> {
    let mut keys = Vec::new();
    if look == Look::Widgets {
        let widgets = reader.widget_ids()?;
        reader.each_setting(
            "",
            Fields::Light,
            |row| model::widget_look_covers(row.key(), &widgets),
            |_, s| push(&mut keys, s.key),
        )?;
        return Ok(keys);
    }
    let groups = reader.groups()?;
    reader.each_setting(
        "",
        Fields::Light,
        |_| true,
        |_, s| {
            if model::within(&groups, listed_in(&groups, &s), LOOK_GROUP)
                && !THEME_KEPT.contains(&s.key.as_str())
            {
                push(&mut keys, s.key)?;
            }
            Ok(())
        },
    )?;
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
    for (id, name) in reader.presets(view.look.kind())? {
        push(&mut themes, (id, name))?;
    }
    let covered = look_settings(reader, view.look)?;
    // Only the covered settings are read (their values and whether they are
    // the defaults).
    let mut all: Vec<Setting> = Vec::new();
    reader.each_setting(
        "",
        Fields::Full,
        |row| covered.iter().any(|key| key == row.key()),
        |_, s| push(&mut all, s),
    )?;
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
    let cx = context(host, false)?;
    // Page by page: the settings that match are kept, no others.
    reader.each_setting(
        "",
        Fields::Full,
        |_| true,
        |reader, s| {
            if !listed(reader, &groups, cx, &s, false)? {
                return Ok(());
            }
            let help = reader.detail(&s.key)?.help;
            if !model::matches(&concat(&[&s.title, " ", &help])?, &words)? {
                return Ok(());
            }
            let group = listed_in(&groups, &s);
            let note = groups
                .iter()
                .find(|g| g.id == group)
                .map_or("", |g| g.title.as_str());
            push(&mut items, Item::noted(&s.title, "›", note)?)?;
            push(
                &mut hits,
                Target::Setting {
                    group: copy(group)?,
                    key: s.key,
                },
            )
        },
    )?;
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
