//! `core.registry` ops 4-10 and 14 against the contract of `registry.md` 2:
//! framing and paging of every list op, detail ops, what each row carries.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;

use super::*;
use crate::abi7::services_wire::service_schema;
use crate::native_host::host_services::{Caller, HostServices};
use alloc::{borrow::Cow, boxed::Box, format, string::String, vec::Vec};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND};
use paperpaw_platform::Instant;
use paperpaw_sdk::{
    ActionSpec, Canvas, Cx, Env, Handled, Id, Manifest, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, SettingGroupSpec, Settings, SlotSpec, Version, keys::InputSpec,
};

struct Quiet;
impl Screen for Quiet {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// One plugin with everything the first ops list.
struct Demo {
    actions: usize,
}

fn id(text: &str) -> Id {
    Id::parse(text).unwrap()
}

impl Plugin for Demo {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("demo"), "Demo name", Version::new(1, 2, 3))
            .description("Shows things.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            id("demo.main"),
            "demo.screen.main",
            Box::new(|_, _| Box::new(Quiet)),
        ))?;
        r.add_slot(SlotSpec::new(id("demo.stage"), "demo.slot.stage"))?;
        r.bind_slot(id("demo.stage"), id("demo.main"), Priority::DEFAULT)?;
        r.add_action(ActionSpec::new(id("demo.sealed"), "demo.action.sealed").sealed())?;
        for index in 0..self.actions {
            r.add_action(ActionSpec::new(
                id(&format!("demo.act{index:02}")),
                "demo.action.other",
            ))?;
        }
        let mut child = SettingGroupSpec::new(id("demo.child"), "demo.group.child").order(2);
        child.parent = Some(id("demo.top"));
        r.add_setting_group(SettingGroupSpec::new(id("demo.top"), "demo.group.top").order(1))?;
        r.add_setting_group(child)?;
        r.add_input(InputSpec::new(id("demo.chord"), "demo.input.chord"))?;
        Ok(())
    }
}

/// A second, bare plugin.
struct Bare;
impl Plugin for Bare {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("bare"), "Bare", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, _: &mut Registrar) -> Result<(), RegisterError> {
        Ok(())
    }
}

fn kernel(actions: usize) -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Demo { actions })
        .plugin(Bare)
        .build()
        .unwrap()
}

struct View<'a> {
    env: &'a Env<'a>,
    sources: Sources<'a>,
    hosts: HostServices,
    registry: bool,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn registry(&self) -> Result<&dyn RegistryView, i32> {
        if self.registry {
            Ok(&self.sources)
        } else {
            Err(ERR_UNSUPPORTED)
        }
    }
}

pub(super) fn op(id: u32) -> &'static ServiceOp {
    service_schema(schema::KEY, schema::MAJOR)
        .unwrap()
        .op(id)
        .unwrap()
}

/// One call of op `id` with `input` (a counted batch), on `kernel`.
fn call(kernel: &crate::Kernel, id: u32, input: &[u8], room: usize) -> Result<Vec<u8>, i32> {
    call_in(kernel, None, id, input, room)
}

/// [`call`] with a declaration store.
pub(super) fn call_in(
    kernel: &crate::Kernel,
    directory: Option<&crate::native_host::native_declarations::NativeDeclarations>,
    id: u32,
    input: &[u8],
    room: usize,
) -> Result<Vec<u8>, i32> {
    let env = kernel.shared.env(Instant(0));
    let view = View {
        env: &env,
        sources: Sources {
            env: &env,
            directory,
            generation: &kernel.shared.registry_generation,
        },
        hosts: HostServices::new(),
        registry: true,
    };
    let caller = Caller {
        owner: 7,
        plugin: "reader",
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
        hosts: &view.hosts,
    };
    let mut out = std::vec![0xaa; room];
    let used = backend(&view, &caller, op(id), input, &mut out)?;
    out.truncate(used);
    Ok(out)
}

fn query(scope: &str, cursor: u32, generation: u32) -> Vec<u8> {
    query_flags(scope, cursor, generation, 0)
}

pub(super) fn query_flags(scope: &str, cursor: u32, generation: u32, flags: u32) -> Vec<u8> {
    let record = Record::ServiceRegQuery(wire::ServiceRegQuery {
        scope,
        cursor,
        generation,
        flags,
    });
    let mut input = std::vec![0; 512];
    let used = wire::records_encode_counted(&[record], 1, 256, &mut input).unwrap();
    input.truncate(used);
    input
}

pub(super) fn key(key: &str) -> Vec<u8> {
    let record = Record::ServiceRegKey(wire::ServiceRegKey { key });
    let mut input = std::vec![0; 512];
    let used = wire::records_encode_counted(&[record], 1, 256, &mut input).unwrap();
    input.truncate(used);
    input
}

/// A reply as text rows, one per record, and its `end`.
pub(super) fn read(reply: &[u8]) -> (Vec<String>, Option<wire::ServiceEnd>) {
    let mut rows = Vec::new();
    let mut end = None;
    let mut rest = reply;
    while !rest.is_empty() {
        let (record, used) = wire::record_decode_prefix(rest).unwrap();
        rest = &rest[used..];
        assert!(end.is_none(), "nothing follows the end");
        rows.push(match record {
            Record::ServiceEnd(e) => {
                end = Some(e);
                continue;
            }
            Record::ServiceRegItem(r) => {
                format!("item {} {} {} {}", r.id, r.plugin, r.flags, r.title)
            }
            Record::ServiceRegGroup(r) => format!(
                "group {} parent={} page={} order={} {}",
                r.id, r.parent, r.page, r.order, r.title
            ),
            Record::ServiceRegSlot(r) => format!(
                "slot {} screen={} default={} flags={} {}",
                r.id, r.screen, r.default, r.flags, r.title
            ),
            Record::ServiceRegPlugin(r) => format!(
                "plugin {} {} {} kind={} state={} flags={} problem={}/{} update={}",
                r.id,
                r.name,
                r.version,
                r.kind,
                r.state,
                r.flags,
                r.problem,
                r.problem_arg,
                r.update
            ),
            Record::ServiceRegText(r) => format!("text {}", r.text),
            Record::ServiceRegValue(r) => format!("value {} {:?} {}", r.key, r.value, r.flags),
            Record::ServiceRegMenuRow(r) => format!(
                "row {} kind={} flags={} order={} by={} title={} icon={} badge={} describe={}",
                r.item, r.kind, r.flags, r.order, r.by, r.title, r.icon, r.badge, r.describe
            ),
            Record::ServiceRegBinding(r) => {
                format!("bind {} {} {} {}", r.layer, r.input, r.action, r.origin)
            }
            Record::ServiceRegSetupStep(r) => format!(
                "step {} {} {} {} {} {}",
                r.id, r.page_kind, r.page, r.order, r.flags, r.title
            ),
            Record::ServiceRegWidget(r) if r.previews.is_empty() => format!(
                "widget {} {} {} {} {} cells={} {}",
                r.id, r.plugin, r.kind, r.icon, r.flags, r.cells, r.title
            ),
            Record::ServiceRegWidget(r) => format!(
                "widget {} {} {} {} {} cells={} previews={} {}",
                r.id, r.plugin, r.kind, r.icon, r.flags, r.cells, r.previews, r.title
            ),
            Record::ServiceRegStatus(r) => format!(
                "status {} {} kind={} flags={} priority={} size={} tap={} {}",
                r.id, r.plugin, r.kind, r.flags, r.priority, r.size, r.tap, r.title
            ),
            other => panic!("unexpected {other:?}"),
        });
    }
    (rows, end)
}

pub(super) fn page(
    kernel: &crate::Kernel,
    id: u32,
    scope: &str,
) -> (Vec<String>, wire::ServiceEnd) {
    let reply = call(kernel, id, &query(scope, 0, 0), 4096).unwrap();
    let (rows, end) = read(&reply);
    (rows, end.unwrap())
}

/// Every row of a list, page by page.
pub(super) fn pages(kernel: &crate::Kernel, id: u32, scope: &str) -> Vec<String> {
    let mut all = Vec::new();
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = call(kernel, id, &query(scope, cursor, generation), 4096).unwrap();
        let (rows, end) = read(&reply);
        let end = end.unwrap();
        all.extend(rows);
        if end.flags & 1 != 0 {
            return all;
        }
        (cursor, generation) = (end.next, end.generation);
    }
}

#[test]
fn a_frame_without_a_registry_refuses_every_op() {
    let kernel = kernel(0);
    let env = kernel.shared.env(Instant(0));
    let view = View {
        env: &env,
        sources: Sources {
            env: &env,
            directory: None,
            generation: &kernel.shared.registry_generation,
        },
        hosts: HostServices::new(),
        registry: false,
    };
    let caller = Caller {
        owner: 7,
        plugin: "reader",
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
        hosts: &view.hosts,
    };
    let mut out = [0; 4096];
    assert_eq!(
        backend(&view, &caller, op(5), &query("", 0, 0), &mut out),
        Err(ERR_UNSUPPORTED)
    );
}

#[test]
fn plugins_start_with_the_host_and_list_every_plugin_once() {
    let kernel = kernel(0);
    let (rows, end) = page(&kernel, schema::OP_PLUGINS, "");
    assert!(rows[0].starts_with("plugin core "), "{rows:?}");
    let ids: Vec<&str> = rows
        .iter()
        .map(|row| row.split(' ').nth(1).unwrap())
        .collect();
    assert_eq!(ids.iter().filter(|id| **id == "core").count(), 1);
    assert!(ids.contains(&"demo") && ids.contains(&"bare"));
    assert_eq!(
        (end.flags, end.total, end.next),
        (1, rows.len() as u32, rows.len() as u32)
    );
    let demo = rows
        .iter()
        .find(|row| row.starts_with("plugin demo "))
        .unwrap();
    // Name, version, built in, running, enabled, no problem, no update.
    assert_eq!(
        demo,
        "plugin demo Demo name 1.2.3 kind=0 state=0 flags=9 problem=0/0 update="
    );
}

#[test]
fn a_plugin_is_one_row_and_its_description_and_an_unknown_one_is_not_found() {
    let kernel = kernel(0);
    let reply = call(&kernel, schema::OP_PLUGIN, &key("demo"), 4096).unwrap();
    let (rows, end) = read(&reply);
    assert!(end.is_none(), "a detail op has no end");
    assert_eq!(rows.len(), 2);
    assert!(rows[0].starts_with("plugin demo "));
    assert_eq!(rows[1], "text Shows things.");
    // Bare has no description: the row alone.
    let reply = call(&kernel, schema::OP_PLUGIN, &key("bare"), 4096).unwrap();
    assert_eq!(read(&reply).0.len(), 1);
    assert_eq!(
        call(&kernel, schema::OP_PLUGIN, &key("nobody"), 4096),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn names_and_descriptions_follow_the_ui_language() {
    let mut kernel = kernel(0);
    kernel.shared.translator.add(
        paperpaw_i18n::Catalog::parse(Cow::Borrowed(
            "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\n\
             demo.plugin.name = Vorfuehrung\ndemo.plugin.description = Zeigt Dinge.\n\
             demo.action.sealed = Versiegelt\n",
        ))
        .unwrap(),
    );
    kernel.shared.translator.set_language("de");
    let reply = call(&kernel, schema::OP_PLUGIN, &key("demo"), 4096).unwrap();
    let (rows, _) = read(&reply);
    assert!(
        rows[0].starts_with("plugin demo Vorfuehrung 1.2.3 "),
        "{rows:?}"
    );
    assert_eq!(rows[1], "text Zeigt Dinge.");
    let (rows, _) = page(&kernel, schema::OP_ACTIONS, "demo");
    assert_eq!(rows[0], "item demo.sealed demo 2 Versiegelt");
}

#[test]
fn actions_and_screens_are_listed_by_plugin_with_their_titles() {
    let kernel = kernel(1);
    let (rows, end) = page(&kernel, schema::OP_ACTIONS, "demo");
    assert_eq!(
        rows,
        [
            "item demo.act00 demo 0 demo.action.other",
            "item demo.sealed demo 2 demo.action.sealed",
        ]
    );
    assert_eq!((end.flags, end.total), (1, 2));
    let (all, _) = page(&kernel, schema::OP_ACTIONS, "");
    assert!(all.len() > 2, "the host's own actions are listed too");
    let (rows, _) = page(&kernel, schema::OP_SCREENS, "demo");
    assert_eq!(rows, ["item demo.main demo 0 demo.screen.main"]);
    // A scope that names nothing is an empty page, not an error.
    let (rows, end) = page(&kernel, schema::OP_SCREENS, "nobody");
    assert!(rows.is_empty());
    assert_eq!((end.flags, end.total), (1, 0));
}

#[test]
fn groups_carry_their_parent_page_and_order() {
    let kernel = kernel(0);
    let (rows, _) = page(&kernel, schema::OP_GROUPS, "demo.top");
    assert_eq!(
        rows,
        ["group demo.child parent=demo.top page= order=2 demo.group.child"]
    );
    let (rows, _) = page(&kernel, schema::OP_GROUPS, "");
    assert!(rows.contains(&"group demo.top parent= page= order=1 demo.group.top".into()));
    assert!(rows.iter().any(|row| row.starts_with("group demo.child ")));
}

#[test]
fn a_slot_shows_its_bound_screen_and_its_offers() {
    let kernel = kernel(0);
    let (rows, _) = page(&kernel, schema::OP_SLOTS, "");
    assert!(
        rows.contains(
            &"slot demo.stage screen=demo.main default=demo.main flags=0 demo.slot.stage".into()
        ),
        "{rows:?}"
    );
    let (rows, _) = page(&kernel, schema::OP_SLOT_OFFERS, "demo.stage");
    assert_eq!(rows, ["item demo.main demo 0 demo.screen.main"]);
    let (rows, end) = page(&kernel, schema::OP_SLOT_OFFERS, "no.slot");
    assert!(rows.is_empty() && end.total == 0);
}

#[test]
fn inputs_list_the_device_then_the_plugins_own() {
    let kernel = kernel(0);
    let rows = pages(&kernel, schema::OP_INPUTS, "");
    // The test device has a touch screen and no named buttons.
    assert!(
        rows.contains(&"item tap.center  0 tap.center".into()),
        "{rows:?}"
    );
    assert!(rows.contains(&"item swipe.left  4 swipe.left".into()));
    assert_eq!(
        rows.last().unwrap(),
        "item input.demo.chord demo 16 demo.input.chord"
    );
}

#[test]
fn pages_continue_by_cursor_under_one_generation() {
    let kernel = kernel(40);
    let first = call(&kernel, schema::OP_ACTIONS, &query("demo", 0, 0), 4096).unwrap();
    let (rows, end) = read(&first);
    let end = end.unwrap();
    assert_eq!(rows.len(), 32);
    assert_eq!((end.flags, end.total, end.next), (0, 41, 32));
    assert_ne!(end.generation, 0);
    let second = call(
        &kernel,
        schema::OP_ACTIONS,
        &query("demo", end.next, end.generation),
        4096,
    )
    .unwrap();
    let (more, last) = read(&second);
    let last = last.unwrap();
    assert_eq!(more.len(), 9);
    assert_eq!(
        (last.flags, last.next, last.generation),
        (1, 41, end.generation)
    );
    // The rows of the two pages are all of the list, once.
    let mut all: Vec<String> = rows.into_iter().chain(more).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 41);
}

#[test]
fn a_stale_generation_or_a_bare_cursor_is_a_state_error_and_touches_nothing() {
    let kernel = kernel(2);
    let env = kernel.shared.env(Instant(0));
    let current = Sources {
        env: &env,
        directory: None,
        generation: &kernel.shared.registry_generation,
    }
    .generation();
    for input in [
        query("demo", 1, 0),
        query("demo", 0, current.wrapping_add(1)),
    ] {
        let env = kernel.shared.env(Instant(0));
        let view = View {
            env: &env,
            sources: Sources {
                env: &env,
                directory: None,
                generation: &kernel.shared.registry_generation,
            },
            hosts: HostServices::new(),
            registry: true,
        };
        let caller = Caller {
            owner: 7,
            plugin: "reader",
            features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
            hosts: &view.hosts,
        };
        let mut out = [0xaa_u8; 4096];
        assert_eq!(
            backend(&view, &caller, op(schema::OP_ACTIONS), &input, &mut out),
            Err(ERR_STATE)
        );
        assert!(out.iter().all(|byte| *byte == 0xaa));
    }
    // A cursor past the end is invalid, the cursor at the end an empty page.
    assert_eq!(
        call(
            &kernel,
            schema::OP_ACTIONS,
            &query("demo", 4, current),
            4096
        ),
        Err(ERR_INVALID)
    );
    let reply = call(
        &kernel,
        schema::OP_ACTIONS,
        &query("demo", 3, current),
        4096,
    )
    .unwrap();
    let (rows, end) = read(&reply);
    assert!(rows.is_empty());
    assert_eq!((end.unwrap().flags, rows.len()), (1, 0));
}

#[test]
fn the_generation_is_the_kernels_counter_and_a_change_of_the_same_size_bumps_it() {
    let mut kernel = kernel(0);
    let generation = |kernel: &crate::Kernel| kernel.shared.registry_generation.get();
    let before = generation(&kernel);
    assert_ne!(before, 0);
    // Nothing changed: the same generation, however often it is read.
    assert_eq!(generation(&kernel), before);
    // A plugin switched off: the list has as many rows as before, and a new
    // generation.
    let count = page(&kernel, schema::OP_PLUGINS, "").1.total;
    kernel.shared.plugin_mut("demo").unwrap().enabled = false;
    assert_eq!(page(&kernel, schema::OP_PLUGINS, "").1.total, count);
    let after = generation(&kernel);
    assert_ne!(after, before);
    // The page ends with it, so a reader holding the old one is told so.
    let (_, end) = page(&kernel, schema::OP_PLUGINS, "");
    assert_eq!(end.generation, after);
    assert_eq!(
        call(&kernel, schema::OP_PLUGINS, &query("", 1, before), 4096),
        Err(ERR_STATE)
    );
}

/// A plugin whose id does not fit `reg_plugin.id` (64 bytes): no check of the
/// manifest bounds it.
struct LongId;
impl Plugin for LongId {
    fn manifest(&self) -> Manifest {
        let id = format!("long-{}", "x".repeat(70));
        Manifest::new(Id::parse(&id).unwrap(), "Long", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, _: &mut Registrar) -> Result<(), RegisterError> {
        Ok(())
    }
}

#[test]
fn a_row_that_cannot_be_framed_is_skipped_by_every_walk() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Bare)
        .plugin(LongId)
        .plugin(Demo { actions: 0 })
        .build()
        .unwrap();
    let rows = pages(&kernel, schema::OP_PLUGINS, "");
    let ids: Vec<&str> = rows
        .iter()
        .map(|row| row.split(' ').nth(1).unwrap())
        .collect();
    assert!(ids.contains(&"bare") && ids.contains(&"demo"));
    assert!(!ids.iter().any(|id| id.starts_with("long-")), "{ids:?}");
    // The count, the cursor and the rows agree: every page is whole and the
    // last one says so.
    let (page_rows, end) = page(&kernel, schema::OP_PLUGINS, "");
    assert_eq!(end.total as usize, page_rows.len());
    assert_eq!((end.flags, end.next), (1, end.total));
    // Asking for the one plugin that does not fit is an error, not a cut id.
    assert_eq!(
        call(
            &kernel,
            schema::OP_PLUGIN,
            &key(&format!("long-{}", "x".repeat(70))),
            4096
        ),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn a_reply_buffer_too_small_for_the_end_is_no_space_and_other_ops_are_not_here_yet() {
    let kernel = kernel(0);
    assert_eq!(
        call(&kernel, schema::OP_PLUGINS, &query("", 0, 0), 8),
        Err(ERR_NO_SPACE)
    );
    // Needs (op 20) follows once the host keeps what plugins require.
    assert_eq!(
        call(&kernel, schema::OP_NEEDS, &query("demo", 0, 0), 4096),
        Err(ERR_UNSUPPORTED)
    );
}

/// The kernel with data plugin `names` on its storage, each `(id, extra)`:
/// `extra` goes into the `[plugin]` section of its `plugin.ini`.
fn data_kernel(plugins: &[(&str, &str)]) -> crate::Kernel {
    let mut storage = paperpaw_sdk::MemoryStorage::new();
    for (name, extra) in plugins {
        let ini = format!("[plugin]\nname = {name}\nformats = 4\n{extra}\n");
        storage = storage.with(&format!("plugins/{name}/plugin.ini"), ini);
    }
    crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Demo { actions: 1 })
        .storage(storage)
        .build()
        .unwrap()
}

fn generation(kernel: &crate::Kernel) -> u32 {
    kernel.shared.registry_generation.get()
}

/// Whether `command` changed the generation, by the kernel's own route.
fn bumps(kernel: &mut crate::Kernel, command: paperpaw_sdk::Command) -> bool {
    let before = generation(kernel);
    kernel.apply(command);
    generation(kernel) != before
}

#[test]
fn every_change_of_a_list_bumps_the_generation() {
    use paperpaw_sdk::Command;
    let mut kernel = data_kernel(&[("cosy", ""), ("other", "")]);
    assert_ne!(generation(&kernel), 0);
    // Disabled, enabled.
    assert!(bumps(
        &mut kernel,
        Command::SetPluginEnabled(id("cosy"), false)
    ));
    assert!(bumps(
        &mut kernel,
        Command::SetPluginEnabled(id("cosy"), true)
    ));
    // Removed.
    assert!(bumps(&mut kernel, Command::RemovePlugin(id("other"))));
    assert!(kernel.plugins().get("other").is_none());
    // Installed (a folder staged as the shell does), updated over itself.
    for version in ["1.0.0", "1.1.0"] {
        kernel
            .shared
            .storage
            .write(
                "plugins/.fresh.part/plugin.ini",
                format!("[plugin]\nname = Fresh\nid = fresh\nversion = {version}\nformats = 4\n")
                    .as_bytes(),
            )
            .unwrap();
        assert!(bumps(
            &mut kernel,
            Command::InstallPlugin(id("fresh"), "plugins/.fresh.part".into())
        ));
    }
    // Re-read from a folder.
    kernel
        .shared
        .storage
        .write(
            "plugins/again/plugin.ini",
            b"[plugin]\nname = Again\nformats = 4\n",
        )
        .unwrap();
    assert!(bumps(
        &mut kernel,
        Command::LoadPlugin("plugins/again".into())
    ));
    // A slot bound, a key bound, a menu set.
    assert!(bumps(
        &mut kernel,
        Command::BindSlot(id("demo.stage"), Some(id("demo.main")))
    ));
    assert!(bumps(
        &mut kernel,
        Command::SetMenu(id("demo.menu"), Some(Vec::new()))
    ));
    assert!(bumps(
        &mut kernel,
        Command::bind("base", "up", Some(id("demo.act00")))
    ));
}

#[test]
fn a_plugin_listed_by_the_host_and_a_declaration_batch_bump_the_generation() {
    // Listed, as a plugin in safe mode or one not loaded is.
    let mut kernel = kernel(0);
    let before = generation(&kernel);
    kernel.add_plugin_info(
        Manifest::new(id("later"), "Later", Version::new(0, 1, 0)),
        paperpaw_sdk::PluginState::Off,
        false,
    );
    let listed = generation(&kernel);
    assert_ne!(listed, before);
    // A batch of declarations, inputs among them.
    let mut staging = crate::registry::Staging::default();
    staging
        .inputs
        .push(InputSpec::new(id("later.chord"), "later.input.chord"));
    kernel.merge(&id("later"), staging).unwrap();
    assert_ne!(generation(&kernel), listed);
}

#[test]
fn a_plugin_refused_at_a_declaration_cap_says_which_while_it_is_failed() {
    use crate::native_host::declaration_checks::Cap;
    use paperpaw_sdk::PluginState;
    let mut kernel = kernel(0);
    for name in ["filer", "reader", "other"] {
        kernel.add_plugin_info(
            Manifest::new(Id::new_static(name), name, Version::new(1, 0, 0)),
            PluginState::Failed,
            true,
        );
    }
    let directory = crate::native_host::native_declarations::NativeDeclarations::default();
    // A 33rd format: 14 with the device's 32; a 5th reader: 15 with 4.
    directory.record_refusal("filer", Cap::of(Cap::FORMATS, 32));
    directory.record_refusal("reader", Cap::of(Cap::READERS, 4));
    // A running plugin keeps its own row (a refusal kept for it is not shown).
    directory.record_refusal("demo", Cap::of(Cap::FORMATS, 8));
    let reply = call_in(
        &kernel,
        Some(&directory),
        schema::OP_PLUGINS,
        &query("", 0, 0),
        4096,
    )
    .unwrap();
    let (rows, _) = read(&reply);
    let row = |id: &str| {
        let prefix = format!("plugin {id} ");
        rows.iter()
            .find(|row| row.starts_with(&prefix))
            .unwrap()
            .clone()
    };
    assert!(row("filer").contains("state=2"), "{}", row("filer"));
    assert!(row("filer").contains("problem=14/32"), "{}", row("filer"));
    assert!(row("reader").contains("problem=15/4"), "{}", row("reader"));
    assert!(row("other").contains("problem=0/0"), "{}", row("other"));
    assert!(row("demo").contains("problem=0/0"), "{}", row("demo"));
    // Admitted again (or tried again): what was kept goes.
    directory.clear_refusal("filer");
    let reply = call_in(
        &kernel,
        Some(&directory),
        schema::OP_PLUGINS,
        &query("", 0, 0),
        4096,
    )
    .unwrap();
    assert!(
        read(&reply)
            .0
            .iter()
            .any(|r| r.starts_with("plugin filer ") && r.contains("problem=0/0"))
    );
}

#[test]
fn a_removed_plugin_leaves_no_refusal_behind() {
    use crate::native_host::declaration_checks::Cap;
    let mut kernel = kernel(0);
    kernel.shared.registry.native_declarations = Some(
        crate::native_host::shared::Shared::try_new(
            crate::native_host::native_declarations::NativeDeclarations::default(),
        )
        .unwrap(),
    );
    fn kept(kernel: &crate::Kernel) -> bool {
        let declared = kernel.shared.registry.native_declarations.as_ref().unwrap();
        declared.refusal("demo").is_some()
    }
    let declared = kernel.shared.registry.native_declarations.as_ref().unwrap();
    declared.record_refusal("demo", Cap::of(Cap::FORMATS, 8));
    assert!(kept(&kernel));
    kernel.unload_plugin(&Id::new_static("demo"));
    assert!(!kept(&kernel));
}

#[test]
fn a_plugin_a_store_installed_says_so_with_its_release_as_the_store_named_it() {
    // The host's record: a store put in place cosy 2.1 and mine 1.0.0 (since
    // replaced by another way's 2.0.0); a line naming a built-in counts for
    // nothing.
    let record = "cosy\t1.0.0\ncosy\t2.1\nmine\t1.0.0\ndemo\t1.2.3\n";
    let mut storage =
        paperpaw_sdk::MemoryStorage::new().with(".paperpaw/plugin-origin.txt", record);
    for (name, version) in [("cosy", "2.1"), ("mine", "2.0.0"), ("own", "2.1")] {
        let ini = format!("[plugin]\nname = {name}\nversion = {version}\nformats = 4\n");
        storage = storage.with(&format!("plugins/{name}/plugin.ini"), ini);
    }
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Demo { actions: 1 })
        .storage(storage)
        .build()
        .unwrap();
    let (rows, _) = page(&kernel, schema::OP_PLUGINS, "");
    let row = |id: &str| {
        let head = format!("plugin {id} ");
        rows.iter()
            .find(|row| row.starts_with(&head))
            .unwrap()
            .clone()
    };
    // Enabled (bit 0) and installed by a store (bit 5), as it was named.
    assert!(
        row("cosy").contains(" 2.1 kind=1 state=0 flags=33 "),
        "{rows:?}"
    );
    // Replaced since, put in place otherwise, built in: no bit 5.
    assert!(
        row("mine").contains(" 2.0.0 kind=1 state=0 flags=1 "),
        "{rows:?}"
    );
    assert!(
        row("own").contains(" 2.1.0 kind=1 state=0 flags=1 "),
        "{rows:?}"
    );
    assert!(row("demo").contains(" flags=9 "), "{rows:?}");
    // Op 6 says the same.
    let reply = call(&kernel, schema::OP_PLUGIN, &key("cosy"), 4096).unwrap();
    assert!(read(&reply).0[0].contains(" 2.1 kind=1 state=0 flags=33 "));
    // Without room for the record the call is refused (asked again, it answers).
    use crate::native_host::shell::tests::FAIL_SIZE;
    FAIL_SIZE.with(|size| size.set(record.len()));
    let refused = call(&kernel, schema::OP_PLUGINS, &query("", 0, 0), 4096);
    let unspent = FAIL_SIZE.with(|size| size.replace(0));
    assert_eq!((refused.err(), unspent), (Some(ERR_NO_MEMORY), 0));
}

#[test]
fn the_first_plugin_rows_take_a_device_without_a_record_for_the_stores() {
    use crate::native_host::store_record::tests::record;
    // Installed before PaperPaw kept a record of the store's installs.
    let ini = "[plugin]\nname = cosy\nversion = 2.1\nformats = 4\n";
    let storage = paperpaw_sdk::MemoryStorage::new().with("plugins/cosy/plugin.ini", ini);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Demo { actions: 1 })
        .storage(storage)
        .build()
        .unwrap();
    let card = kernel.shared.storage.as_ref();
    assert_eq!(record(card), None);
    let (rows, _) = page(&kernel, schema::OP_PLUGINS, "");
    let cosy = rows.iter().find(|row| row.starts_with("plugin cosy "));
    assert!(
        cosy.unwrap().contains(" kind=1 state=0 flags=33 "),
        "{rows:?}"
    );
    // Built-in plugins are never the store's.
    assert_eq!(record(card).as_deref(), Some("cosy\t2.1.0\n"));
}
