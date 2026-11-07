//! The flat Buttons and gestures plugin: a list of inputs, a place chooser and
//! an action picker. Views own copies; the host's registry stays the truth.
use crate::model::{self, POLICY, Place};
use crate::native_data::{self, Named, Reader};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    keys::POLICIES,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, KeyBinding, RedrawMode},
        events::{AnswerOutcome, Event},
        formatting::Arg,
        registrations,
        reply::Reply,
        ui::{Instance, Notify, Open, Param, Question, QuestionKind, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Item, ItemList, ItemStep, Messages, Screens,
    items::RowItem,
    mem::{self, SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
    uitext,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/keys_declarations.rs"));
}

const OPEN: u32 = 1;
const EVENTS: u32 = 2;
const PICK: u32 = 10;
const PLACE: u32 = 11;
const LIST: u32 = 12;

/// The longest search words the picker takes.
const SEARCH_LIMIT: usize = 256;

/// What an open question is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// The search words of the picker.
    Search(Instance),
    /// Taking every change back.
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Heading,
    Place,
    Policy,
    Input(String),
    Reset,
}

/// The list of inputs.
#[derive(Debug)]
struct ListView {
    inputs: Vec<Named>,
    touch: bool,
    places: Vec<Place>,
    place: usize,
    rows: Vec<Row>,
    /// The index into the policies of the place shown.
    policy: usize,
    list: ItemList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pick {
    Heading,
    Search,
    Standard,
    Action(usize),
}

/// Choosing what an input does.
#[derive(Debug)]
struct PickView {
    input: String,
    layers: Vec<String>,
    title: String,
    query: String,
    actions: Vec<PickAction>,
    entries: Vec<Pick>,
    list: ItemList,
}

#[derive(Debug)]
struct PickAction {
    id: String,
    title: String,
    group: u8,
}

/// Choosing where changes apply.
#[derive(Debug)]
struct PlaceView {
    places: Vec<Place>,
    list: ItemList,
}

#[derive(Debug)]
enum View {
    List(ListView),
    Pick(PickView),
    Place(PlaceView),
}

/// The interface-7 Buttons and gestures plugin.
#[derive(Debug, Default)]
pub struct NativeKeys {
    ui: Option<Ui>,
    screens: Screens<View>,
    asked: Option<(RequestId, Purpose)>,
}

fn screen_token(screen: &str) -> Option<u32> {
    Some(match screen {
        "keys.list" => LIST,
        "keys.pick" => PICK,
        "keys.place" => PLACE,
        _ => return None,
    })
}

impl Plugin for NativeKeys {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        match event {
            Event::ActionRun {
                action: "keys.open",
                ..
            } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("keys.list")?)
            }
            Event::ScreenOpened {
                screen,
                instance,
                handover,
                ..
            } => self.opened(host, screen, instance, handover),
            Event::ScreenShown {
                screen,
                instance: Some(instance),
                ..
            } => {
                if let Some(token) = screen_token(screen) {
                    self.screens.shown(token, instance)?;
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            Event::RegistryChanged { .. } if token == EVENTS => self.changed(host),
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        match self.screens.top(token).ok_or(Error::State)? {
            View::List(view) => {
                let title = messages.copy(host, "keys.title")?;
                view.list.draw(host, &mut frame, &title, true)
            }
            View::Pick(view) => view.list.draw(host, &mut frame, &view.title, true),
            View::Place(view) => {
                let title = messages.copy(host, "keys.place")?;
                view.list.draw(host, &mut frame, &title, true)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let list = match view {
            View::List(v) => &mut v.list,
            View::Pick(v) => &mut v.list,
            View::Place(v) => &mut v.list,
        };
        let step = list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step {
            self.choose(host, token, index)?;
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeKeys {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let (mut chosen, mut layers) = (String::new(), String::new());
        if let Some(handover) = handover {
            for param in self.ui()?.params(host, handover, &mut input, &mut output)? {
                let param = param?;
                match param.key() {
                    "input" => chosen = copy(param.value())?,
                    "layers" => layers = copy(param.value())?,
                    _ => {}
                }
            }
        }
        let mut reader = Reader::open(host)?;
        let view = Self::build(host, &mut reader, screen, chosen, &layers);
        let view = reader.finish(view)?;
        self.screens.open(instance, view)
    }

    fn build(
        host: &Host<'_>,
        reader: &mut Reader<'_, '_>,
        screen: &str,
        input: String,
        layers: &str,
    ) -> Result<View, Error> {
        let mut messages = Messages::new()?;
        match screen {
            "keys.list" => {
                let inputs = reader.inputs()?;
                let mut view = ListView {
                    touch: model::has_touch(&ids(&inputs)?),
                    inputs,
                    places: Vec::new(),
                    place: 0,
                    rows: Vec::new(),
                    policy: 0,
                    list: ItemList::default(),
                };
                let screens = reader.screens()?;
                let (policy, _) = reader.policy_layers_and_changes()?;
                view.places = native_data::places(
                    &mut |key| messages.copy(host, key),
                    &screens,
                    &policy,
                    false,
                )?;
                view.rebuild(host, reader, &mut messages)?;
                Ok(View::List(view))
            }
            "keys.place" => {
                let screens = reader.screens()?;
                let (policy, _) = reader.policy_layers_and_changes()?;
                let mut places = native_data::places(
                    &mut |key| messages.copy(host, key),
                    &screens,
                    &policy,
                    false,
                )?;
                let mut rest = native_data::places(
                    &mut |key| messages.copy(host, key),
                    &screens,
                    &policy,
                    true,
                )?;
                // Every screen that is not listed yet, told apart by kind.
                rest.retain(|p| !places.iter().any(|q| q.layer() == p.layer()));
                let mut kinds = Messages::new()?;
                let mut kind = |layer: &str| -> Result<String, Error> {
                    let last = layer.rsplit('.').next().unwrap_or(layer);
                    match kinds.copy(host, &concat(&["keys.kind.", last])?) {
                        Ok(name) => Ok(name),
                        Err(Error::NoMemory) => Err(Error::NoMemory),
                        Err(_) => mem::replace_char(last, '-', ' '),
                    }
                };
                model::tell_apart(&mut rest, &mut kind)?;
                mem::extend(&mut places, rest)?;
                let items = mem::collect(places.iter().map(|p| Item::row(&p.title, "")))?;
                Ok(View::Place(PlaceView {
                    list: ItemList::new(items),
                    places,
                }))
            }
            "keys.pick" => {
                let inputs = reader.inputs()?;
                let mut view = PickView {
                    title: input_name(host, &mut messages, &inputs, &input)?,
                    input,
                    layers: split_layers(layers)?,
                    query: String::new(),
                    actions: Vec::new(),
                    entries: Vec::new(),
                    list: ItemList::default(),
                };
                view.load(reader)?;
                view.build(host, &mut messages)?;
                Ok(View::Pick(view))
            }
            _ => Err(Error::Invalid),
        }
    }

    /// Rebuilds the open views after bindings changed.
    fn changed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.screens.is_empty() {
            return Ok(());
        }
        let mut reader = Reader::open(host)?;
        let done = (|| {
            let mut messages = Messages::new()?;
            for view in self.screens.all() {
                if let View::List(list) = view {
                    list.rebuild(host, &mut reader, &mut messages)?;
                }
            }
            Ok(())
        })();
        reader.finish(done)?;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    fn choose(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let mut scratch = buffer(SERVICE_BUFFER)?;
        match token {
            LIST => {
                let Some(View::List(view)) = self.screens.top(token) else {
                    return Ok(());
                };
                match view.rows.get(index) {
                    Some(Row::Place) => {
                        CommandSender::new(host, &mut scratch).open(Open::screen("keys.place")?)
                    }
                    Some(Row::Policy) => view.next_policy(host, &mut scratch),
                    Some(Row::Input(input)) => {
                        let layers = mem::join(&view.place().layers, ",")?;
                        let mut output = buffer(SERVICE_BUFFER)?;
                        let params = [Param::new("input", input)?, Param::new("layers", &layers)?];
                        let ui = self.ui.as_ref().ok_or(Error::State)?;
                        let handover = ui.handover(host, &params, &mut scratch, &mut output)?;
                        CommandSender::new(host, &mut scratch)
                            .open(Open::screen("keys.pick")?.handover(handover))
                    }
                    Some(Row::Reset) => self.ask_reset(host),
                    _ => Ok(()),
                }
            }
            PLACE => {
                let Some(View::Place(view)) = self.screens.top(token) else {
                    return Ok(());
                };
                let Some(place) = view.places.get(index) else {
                    return Ok(());
                };
                let place = place.try_clone()?;
                let mut reader = Reader::open(host)?;
                let done = (|| {
                    let mut messages = Messages::new()?;
                    for view in self.screens.all() {
                        if let View::List(list) = view {
                            list.choose_place(place.try_clone()?)?;
                            list.rebuild(host, &mut reader, &mut messages)?;
                        }
                    }
                    Ok(())
                })();
                reader.finish(done)?;
                CommandSender::new(host, &mut scratch).back()
            }
            PICK => self.picked(host, index),
            _ => Ok(()),
        }
    }

    fn ask_reset(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "keys.title")?;
        let question = messages.copy(host, "keys.reset.question")?;
        let yes = messages.copy(host, "keys.option.reset")?;
        let no = messages.copy(host, "keys.option.cancel")?;
        let labels = concat(&[&yes, "\n", &no])?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let asked = Question::confirm(&title, &question)?.confirm_labels(&labels)?;
        let request = self.ui()?.ask(host, asked, &mut input, &mut output)?;
        self.asked = Some((request, Purpose::Reset));
        Ok(())
    }

    fn picked(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(View::Pick(view)) = self.screens.top(PICK) else {
            return Ok(());
        };
        let action = match view.entries.get(index) {
            Some(Pick::Search) => return self.ask_search(host),
            Some(Pick::Standard) => None,
            Some(Pick::Action(usize::MAX)) => Some(String::new()),
            Some(Pick::Action(at)) => Some(copy(&view.actions[*at].id)?),
            _ => return Ok(()),
        };
        let (input, layers) = (copy(&view.input)?, mem::copy_strings(&view.layers)?);
        let mut reader = Reader::open(host)?;
        let facts = (|| Ok((reader.inputs()?, reader.bindings(&layers)?)))();
        let (inputs, bindings) = reader.finish(facts)?;
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let mut messages = Messages::new()?;
        let refusal = model::refused(
            &ids(&inputs)?,
            &bindings,
            &layers,
            &input,
            action.as_deref(),
        )?;
        let mut commands = CommandSender::new(host, &mut scratch);
        if let Some(message) = refusal {
            let text = messages.copy(host, message)?;
            return commands.notify(Notify::new(&text)?);
        }
        let layer = layers.first().map_or(model::GLOBAL, String::as_str);
        let binding = match action.as_deref() {
            None => KeyBinding::Default,
            Some("") => KeyBinding::None,
            Some(id) => KeyBinding::Action(id),
        };
        commands.bind_key(layer, &input, binding)?;
        commands.back()
    }

    fn ask_search(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(View::Pick(view)) = self.screens.top(PICK) else {
            return Ok(());
        };
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "keys.search")?;
        let query = copy(&view.query)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self
            .ui()?
            .ask(host, Question::text(&title, "")?, &mut input, &mut output)?;
        // The words so far, as the initial text.
        if !query.is_empty() {
            self.ui()?.text_write(
                host,
                request.text(),
                0,
                &query,
                paperpaw_sdk::native::ui::TextWrite::Last,
                &mut input,
                &mut output,
            )?;
        }
        let instance = self.top_pick_instance()?;
        self.asked = Some((request, Purpose::Search(instance)));
        Ok(())
    }

    fn top_pick_instance(&mut self) -> Result<Instance, Error> {
        self.screens.top_instance(PICK).ok_or(Error::State)
    }

    fn answered(
        &mut self,
        host: &Host<'_>,
        answer: paperpaw_sdk::native::events::Answered,
    ) -> Result<(), Error> {
        let Some((request, purpose)) = self.asked else {
            return Ok(());
        };
        if request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match purpose {
            Purpose::Reset if answer.kind() == QuestionKind::Confirm && answer.choice() == 1 => {
                self.reset(host)
            }
            Purpose::Search(instance) => {
                let words = uitext::read_all(self.ui()?, host, request.text(), SEARCH_LIMIT)?;
                let query = copy(words.trim())?;
                let mut messages = Messages::new()?;
                if let Some(View::Pick(view)) = self.screens.get(instance) {
                    view.query = query;
                    view.build(host, &mut messages)?;
                }
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
            }
            Purpose::Reset => Ok(()),
        }
    }

    /// Every change taken back to what the device came with.
    fn reset(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let mut reader = Reader::open(host)?;
        let changes = reader.policy_layers_and_changes();
        let changes = reader.finish(changes);
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        let key = match &changes {
            Ok(_) => "keys.reset.done",
            Err(_) => "keys.reset.unreadable",
        };
        if let Ok((_, changes)) = &changes {
            for (layer, input) in changes {
                commands.bind_key(layer, input, KeyBinding::Default)?;
            }
        }
        let text = messages.copy(host, key)?;
        commands.notify(Notify::new(&text)?)?;
        commands.queue_redraw(RedrawMode::Partial)
    }
}

fn ids(inputs: &[Named]) -> Result<Vec<String>, Error> {
    mem::collect(inputs.iter().map(|i| copy(&i.id)))
}

fn split_layers(layers: &str) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    for layer in layers.split(',').filter(|l| !l.is_empty()) {
        found.try_reserve(1).map_err(|_| Error::NoMemory)?;
        found.push(copy(layer)?);
    }
    Ok(found)
}

/// An input's name: "Page forward", "Hold: Menu", "Tap: top left".
fn input_name(
    host: &Host<'_>,
    messages: &mut Messages,
    inputs: &[Named],
    input: &str,
) -> Result<String, Error> {
    let (kind, name) = input.split_once('.').unwrap_or((input, ""));
    let part = |messages: &mut Messages, prefix: &str| {
        messages.copy(host, &concat(&["keys.", prefix, ".", name])?)
    };
    let with = |messages: &mut Messages, key: &str, arg: &'static str, value: &str| {
        messages.format(host, key, &[Arg::Text { name: arg, value }])
    };
    match kind {
        "button" => part(messages, "button"),
        "long" | "double" => {
            let button = part(messages, "button")?;
            with(messages, &concat(&["keys.", kind])?, "input", &button)
        }
        "tap" | "double-tap" | "long-tap" => {
            let zone = part(messages, "zone")?;
            with(messages, &concat(&["keys.", kind])?, "zone", &zone)
        }
        "tap2" => messages.copy(host, "keys.tap2"),
        "swipe" | "edge" | "pinch" => part(messages, kind),
        "input" | "gesture" => match inputs.iter().find(|i| i.id == input) {
            Some(found) => copy(&found.title),
            None => copy(input),
        },
        _ => copy(input),
    }
}

impl ListView {
    fn place(&self) -> &Place {
        &self.places[self.place.min(self.places.len().saturating_sub(1))]
    }

    fn choose_place(&mut self, place: Place) -> Result<(), Error> {
        match self.places.iter().position(|p| p.layer() == place.layer()) {
            Some(at) => self.place = at,
            None => {
                push(&mut self.places, place)?;
                self.place = self.places.len() - 1;
            }
        }
        Ok(())
    }

    fn rebuild(
        &mut self,
        host: &Host<'_>,
        reader: &mut Reader<'_, '_>,
        messages: &mut Messages,
    ) -> Result<(), Error> {
        let layers = mem::copy_strings(&self.place().layers)?;
        let bindings = reader.bindings(&layers)?;
        let actions = reader.actions()?;
        let all = ids(&self.inputs)?;
        let everywhere = self.place().everywhere();
        let mut rows = Vec::new();
        let mut items = Vec::new();
        let mut add = |row: Row, item: Item| -> Result<(), Error> {
            rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
            items.try_reserve(1).map_err(|_| Error::NoMemory)?;
            rows.push(row);
            items.push(item);
            Ok(())
        };
        let help = if self.touch {
            "keys.place.help"
        } else {
            "keys.place.help.buttons"
        };
        add(
            Row::Place,
            Item::noted(
                &messages.copy(host, "keys.place")?,
                &self.place().title,
                &messages.copy(host, help)?,
            )?,
        )?;
        if !everywhere && self.touch {
            let now = model::bound(&bindings, &layers, POLICY)
                .filter(|a| POLICIES.contains(a))
                .unwrap_or(POLICIES[0]);
            self.policy = POLICIES.iter().position(|p| *p == now).unwrap_or(0);
            add(
                Row::Policy,
                Item::noted(
                    &messages.copy(host, "keys.policy")?,
                    &messages.copy(host, &concat(&["keys.", now])?)?,
                    &messages.copy(host, &concat(&["keys.", now, ".help"])?)?,
                )?,
            )?;
        }
        let nothing = messages.copy(host, "core.action.nothing")?;
        for section in model::sections(&all)? {
            add(
                Row::Heading,
                Item::heading(&messages.copy(host, section.heading)?)?,
            )?;
            for input in section.inputs {
                let does = match model::bound(&bindings, &layers, &input) {
                    Some(id) if !id.is_empty() => actions
                        .iter()
                        .find(|a| a.id == id)
                        .map_or_else(|| copy(id), |a| copy(&a.title))?,
                    _ => copy(&nothing)?,
                };
                let name = input_name(host, messages, &self.inputs, &input)?;
                let mut item = Item::noted(&name, "›", &does)?;
                if let Item::Row(RowItem { zone, .. }) = &mut item {
                    *zone = model::zone_index(&input);
                }
                add(Row::Input(input), item)?;
            }
        }
        add(
            Row::Reset,
            Item::noted(
                &messages.copy(host, "keys.reset")?,
                "",
                &messages.copy(host, "keys.reset.help")?,
            )?,
        )?;
        self.rows = rows;
        self.list.replace(items);
        Ok(())
    }

    /// Standard, own, off, and round again.
    fn next_policy(&self, host: &Host<'_>, scratch: &mut [u8]) -> Result<(), Error> {
        CommandSender::new(host, scratch).bind_key(
            self.place().layer(),
            POLICY,
            KeyBinding::Action(POLICIES[(self.policy + 1) % POLICIES.len()]),
        )
    }
}

impl PickView {
    fn load(&mut self, reader: &mut Reader<'_, '_>) -> Result<(), Error> {
        // Nothing, standard and the gesture policies are no actions to pick.
        let groups = [
            [
                "core.reader-menu",
                "core.reader-more",
                "core.reader-panel",
                "core.selection-menu",
            ]
            .as_slice(),
            ["core.tools", "home.actions"].as_slice(),
        ];
        let mut sets: [Vec<String>; 2] = [Vec::new(), Vec::new()];
        for (set, menus) in sets.iter_mut().zip(groups) {
            for menu in menus {
                mem::extend(set, reader.menu_items(menu)?)?;
            }
        }
        for named in reader.actions()? {
            if named.id == "core.nothing" || named.id.starts_with("core.gestures.") {
                continue;
            }
            let group = group_of(&named.id, &sets);
            self.actions.try_reserve(1).map_err(|_| Error::NoMemory)?;
            self.actions.push(PickAction {
                id: named.id,
                title: named.title,
                group,
            });
        }
        mem::stable_sort_by(&mut self.actions, |a, b| {
            a.group
                .cmp(&b.group)
                .then_with(|| paperpaw_ui_kit::text::cmp_lowercase(&a.title, &b.title))
        });
        Ok(())
    }

    fn build(&mut self, host: &Host<'_>, messages: &mut Messages) -> Result<(), Error> {
        let query = mem::lowercase(&self.query)?;
        let words = mem::collect(query.split_whitespace().map(Ok))?;
        let mut entries = Vec::new();
        let mut items = Vec::new();
        let mut add = |entry: Pick, item: Item| -> Result<(), Error> {
            entries.try_reserve(1).map_err(|_| Error::NoMemory)?;
            items.try_reserve(1).map_err(|_| Error::NoMemory)?;
            entries.push(entry);
            items.push(item);
            Ok(())
        };
        add(
            Pick::Search,
            Item::row(&messages.copy(host, "keys.search")?, &self.query)?,
        )?;
        if words.is_empty() {
            add(
                Pick::Standard,
                Item::row(&messages.copy(host, "keys.standard")?, "")?,
            )?;
            add(
                Pick::Action(usize::MAX),
                Item::row(&messages.copy(host, "core.action.nothing")?, "")?,
            )?;
        }
        let mut group = None;
        for (at, action) in self.actions.iter().enumerate() {
            let name = mem::lowercase(&action.title)?;
            if !words.iter().all(|w| name.contains(*w)) {
                continue;
            }
            if group != Some(action.group) {
                group = Some(action.group);
                add(
                    Pick::Heading,
                    Item::heading(&messages.copy(host, GROUPS[usize::from(action.group)])?)?,
                )?;
            }
            add(Pick::Action(at), Item::row(&action.title, "")?)?;
        }
        self.entries = entries;
        self.list.replace(items);
        Ok(())
    }
}

/// Section headings of the picker.
const GROUPS: [&str; 5] = [
    "keys.group.navigation",
    "keys.group.reading",
    "keys.group.tools",
    "keys.group.system",
    "keys.group.more",
];

const NAVIGATION: [&str; 13] = [
    "core.next-page",
    "core.prev-page",
    "core.next",
    "core.previous",
    "core.up",
    "core.down",
    "core.left",
    "core.right",
    "core.confirm",
    "core.back",
    "core.home",
    "core.menu",
    "core.long-confirm",
];

fn group_of(id: &str, sets: &[Vec<String>; 2]) -> u8 {
    if NAVIGATION.contains(&id) {
        0
    } else if sets[0].iter().any(|i| i == id) {
        1
    } else if sets[1].iter().any(|i| i == id) {
        2
    } else if id.starts_with("core.") {
        3
    } else {
        4
    }
}
