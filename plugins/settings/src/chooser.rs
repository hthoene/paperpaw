//! Choosing one of a setting's options, on a page of its own.

use super::*;

// ---- Choosing a value ---------------------------------------------------

#[derive(Debug)]
pub(super) struct Chooser {
    pub(super) spec: Option<SettingSpec>,
    /// (value, display name)
    pub(super) options: Vec<(String, String)>,
    /// More ways to choose, after the options: entries of the menu named
    /// like the setting (action, name).
    pub(super) more: Vec<(Id, String)>,
    pub(super) style: UiStyle,
    pub(super) header: ui::Header,
    pub(super) list: ListView,
}

impl Chooser {
    pub(super) fn new(env: &Env<'_>, id: &str) -> Self {
        let (style, header, list) = frame(env, false);
        let mut chooser = Self {
            spec: None,
            options: Vec::new(),
            more: Vec::new(),
            style,
            header,
            list,
        };
        chooser.read(env, id);
        let current = env.settings().text(id);
        chooser.list.set_focus(
            chooser
                .options
                .iter()
                .position(|(v, _)| v == current)
                .unwrap_or(0),
        );
        chooser
    }

    /// The setting's options and more ways to choose, read anew.
    fn read(&mut self, env: &Env<'_>, id: &str) {
        // Of a placement too (`widgets.clock@2.face`), as that one.
        let spec = env.settings().spec(id).cloned().map(|mut spec| {
            if let Ok(id) = Id::parse(id) {
                spec.id = id;
            }
            spec
        });
        let options: Vec<(String, String)> = match spec.as_ref().map(|s| (&s.kind, s)) {
            Some((SettingKind::Language, _)) => env
                .i18n()
                .languages()
                .into_iter()
                .map(|(tag, name)| (tag.to_string(), name.to_string()))
                .collect(),
            Some((SettingKind::Choice(values), spec)) => values
                .iter()
                .map(|v| (v.to_string(), ui::option_name(env, spec, v)))
                .collect(),
            _ => Vec::new(),
        };
        self.more = spec.as_ref().map_or_else(Vec::new, |spec| {
            env.menus()
                .items(spec.id.as_str())
                .iter()
                .filter(|item| !item.section)
                .map(|item| (item.id.clone(), env.i18n().tr(item.title).to_string()))
                .collect()
        });
        self.spec = spec;
        self.options = options;
    }

    fn len(&self) -> usize {
        self.options.len() + self.more.len()
    }

    pub(super) fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some((action, ..)) = index
            .checked_sub(self.options.len())
            .and_then(|i| self.more.get(i))
        {
            cx.command(Command::run(action.clone()));
            return;
        }
        let (Some(spec), Some((value, _))) = (&self.spec, self.options.get(index)) else {
            return;
        };
        cx.command(Command::Back);
        cx.command(Command::SetSetting(
            spec.id.clone(),
            SettingValue::Text(Cow::Owned(value.clone())),
        ));
    }
}

impl Screen for Chooser {
    /// No pictures on it: whole-screen frames need no extra pass to count
    /// them (see [`Screen::pictures_in_grays`]).
    fn pictures_in_grays(&self) -> bool {
        false
    }

    /// Turned or resized: laid out anew, the focus on its option.
    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let (style, header, fresh) = frame(cx.env(), false);
        // The focus, whether it shows and the page stay with the list.
        self.list.set_area(fresh.area());
        self.list.set_row(fresh.row());
        (self.style, self.header) = (style, header);
        cx.request_redraw();
        Handled::Yes
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let len = self.len();
        let title = self
            .spec
            .as_ref()
            .map_or("", |spec| env.i18n().tr(&spec.title));
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        let (options, more) = (&self.options, &self.more);
        let current = self
            .spec
            .as_ref()
            .map_or("", |spec| env.settings().text(spec.id.as_str()));
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            // Lined up with the options, leading on.
            if let Some((_, name)) = index.checked_sub(options.len()).map(|i| &more[i]) {
                ui::Row::value(name, "›").focused(focused).draw(ui, rect);
                return;
            }
            let (value, name) = &options[index];
            let mark = if value == current { "•" } else { "" };
            ui::Row::value(name, mark).focused(focused).draw(ui, rect);
        });
    }

    fn on_event(&mut self, event: &Id, _params: &Params, cx: &mut Cx<'_, '_>) {
        // Fonts downloaded or removed meanwhile.
        if *event == events::PLUGINS_CHANGED
            && let Some(id) = self.spec.as_ref().map(|s| s.id.to_string())
        {
            let env = *cx.env();
            self.read(&env, &id);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        if let Some(change) = self.list.navigate_in(cx.env(), action, len) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.len());
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.len()) {
            Some(index) => {
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
