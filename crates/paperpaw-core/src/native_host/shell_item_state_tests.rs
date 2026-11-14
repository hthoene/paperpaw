//! `command.set_item_state` (60) through a native callback: checked whole at
//! submit, stored after the callback, gone when the callback fails.
use super::*;
use crate::native_host::{
    commands::{WireCommand, commands_encode},
    item_state::{FAINT, HIDDEN, SWITCH},
};
use paperpaw_native::host_glue::{ERR_FULL, ERR_NOT_FOUND, ERR_UNSUPPORTED};

fn state(item: &str, flags: u32) -> WireCommand<'_> {
    WireCommand::SetItemState {
        item,
        flags,
        label: "",
        label_args: &[],
        badge: "",
        badge_args: &[],
    }
}

fn kernel(settings: usize, directory: bool) -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    const KEYS: [&str; 17] = [
        "fake.s0", "fake.s1", "fake.s2", "fake.s3", "fake.s4", "fake.s5", "fake.s6", "fake.s7",
        "fake.s8", "fake.s9", "fake.s10", "fake.s11", "fake.s12", "fake.s13", "fake.s14",
        "fake.s15", "fake.s16",
    ];
    for key in &KEYS[..settings] {
        kernel
            .shared
            .settings
            .define(paperpaw_sdk::SettingSpec::switch(
                key,
                "setting",
                false,
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }
    if directory {
        kernel.shared.registry.native_declarations = Some(
            crate::native_host::shared::Shared::try_new(
                crate::native_host::native_declarations::NativeDeclarations::default(),
            )
            .unwrap(),
        );
    }
    kernel
}

/// Submits `commands` in one batch from a callback that returns `returned`;
/// the submit status and the callback's result.
pub(super) fn run(
    kernel: &mut crate::Kernel,
    commands: &[WireCommand<'_>],
    returned: i32,
) -> (i32, Result<i32, Error>) {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let mut bytes = [0; 2048];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let submitted = core::cell::Cell::new(i32::MIN);
    let result = kernel.with_native_commands(
        &batch,
        &DispatchDepth::new(),
        crate::native_host::shell::command_dispatch::CommandTarget {
            screen: None,
            hop: 0,
            result: crate::native_host::shell::command_dispatch::CommandResult::Status,
        },
        paperpaw_platform::Instant(0),
        |context, frame| {
            // SAFETY: the retained context structurally pins its environment.
            let scope = unsafe { context.map_unchecked(|value| &value.environment) };
            scope.scope_core(frame, || {
                // SAFETY: the encoded command is readable and disjoint from host state.
                submitted.set(unsafe { context.commands.submit(bytes.as_ptr(), len) });
                if returned == OK {
                    Ok(OK)
                } else {
                    Err(Error::Status(returned))
                }
            })?
        },
    );
    drop(batch);
    finish(&hosted);
    (submitted.get(), result)
}

fn pushed(kernel: &crate::Kernel, item: &str) -> Option<u32> {
    let directory = kernel.shared.registry.native_declarations.as_ref()?;
    let states = directory.items.borrow();
    states
        .get("fake", item)
        .map(crate::native_host::item_state::ItemPush::flags)
}

/// A setting its plugin describes (#248) is an item of its own without a
/// spec made of it: no lack of room for one makes it an item not found.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[test]
fn the_state_of_a_described_setting_s_item_is_pushed_without_its_spec() {
    use crate::native_host::described_settings::tests::{
        define_plugin, no_room_for_a_spec, switch,
    };
    use crate::native_host::shell::tests::FAIL_SIZE;
    let mut kernel = kernel(0, true);
    let records = [switch("lazy", "Lazy", 0)];
    let _shown = define_plugin(&mut kernel.shared.settings, "fake", &records);
    no_room_for_a_spec();
    let (submitted, result) = run(&mut kernel, &[state("lazy", FAINT)], OK);
    let armed = FAIL_SIZE.with(|size| size.replace(0));
    assert_ne!(armed, 0, "a spec was made");
    assert_eq!((submitted, result), (OK, Ok(OK)));
    assert_eq!(pushed(&kernel, "lazy"), Some(FAINT));
}

#[test]
fn a_pushed_state_is_stored_after_the_callback_and_the_default_removes_it() {
    let mut kernel = kernel(1, true);
    let (submitted, result) = run(&mut kernel, &[state("s0", HIDDEN | FAINT)], OK);
    assert_eq!((submitted, result), (OK, Ok(OK)));
    assert_eq!(pushed(&kernel, "s0"), Some(HIDDEN | FAINT));
    let (submitted, _) = run(&mut kernel, &[state("s0", 0)], OK);
    assert_eq!(submitted, OK);
    assert_eq!(pushed(&kernel, "s0"), None, "the default state removes it");
}

#[test]
fn a_callback_that_fails_stores_nothing() {
    let mut kernel = kernel(1, true);
    let (submitted, result) = run(&mut kernel, &[state("s0", HIDDEN)], ERR_INVALID);
    assert_eq!(submitted, OK, "accepted at submit");
    assert!(result.is_err());
    assert_eq!(pushed(&kernel, "s0"), None);
    // The room and the plugin's slot reserved at submit went back.
    let directory = kernel.shared.registry.native_declarations.as_ref().unwrap();
    assert_eq!(directory.item_room.get(), 0);
    assert_eq!(directory.items.borrow().slots(), 0);
}

#[test]
fn a_callback_that_succeeds_gives_its_room_back_and_keeps_the_slot_of_a_state() {
    let mut kernel = kernel(1, true);
    let (submitted, _) = run(&mut kernel, &[state("s0", HIDDEN)], OK);
    assert_eq!(submitted, OK);
    let slots = |kernel: &crate::Kernel| {
        let directory = kernel.shared.registry.native_declarations.as_ref().unwrap();
        assert_eq!(directory.item_room.get(), 0);
        directory.items.borrow().slots()
    };
    assert_eq!(slots(&kernel), 1);
    let (submitted, _) = run(&mut kernel, &[state("s0", 0)], OK);
    assert_eq!(submitted, OK);
    assert_eq!(slots(&kernel), 0, "the last state went");
}

#[test]
fn a_batch_with_a_bad_record_is_refused_whole() {
    let mut kernel = kernel(2, true);
    let refused = |kernel: &mut crate::Kernel, commands: &[WireCommand<'_>], status: i32| {
        let (submitted, _) = run(kernel, commands, OK);
        assert_eq!(submitted, status, "{commands:?}");
        assert_eq!(pushed(kernel, "s0"), None, "nothing of it is kept");
        assert_eq!(pushed(kernel, "s1"), None);
    };
    // An item that is none of the plugin's own.
    refused(
        &mut kernel,
        &[state("s0", HIDDEN), state("nobody", HIDDEN)],
        ERR_NOT_FOUND,
    );
    // Two records of one item in a batch.
    refused(
        &mut kernel,
        &[state("s0", HIDDEN), state("s0", FAINT)],
        ERR_INVALID,
    );
    // A switch on a setting: a Boolean setting shows its own value.
    refused(&mut kernel, &[state("s1", SWITCH)], ERR_INVALID);
    // A label key no catalog has.
    let unknown = WireCommand::SetItemState {
        item: "s0",
        flags: 0,
        label: "no.such.key",
        label_args: &[],
        badge: "",
        badge_args: &[],
    };
    refused(&mut kernel, &[unknown], ERR_NOT_FOUND);
}

fn catalog(kernel: &mut crate::Kernel) {
    let long = "x".repeat(200);
    kernel.shared.translator.add(
        paperpaw_i18n::Catalog::parse(alloc::borrow::Cow::Owned(alloc::format!(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\n\
             fake.label = Hello {{name}}\nfake.plain = Plain\nfake.long = {long}\n"
        )))
        .unwrap(),
    );
}

#[test]
fn label_and_badge_arguments_are_decoded_at_submit() {
    let mut kernel = kernel(1, true);
    catalog(&mut kernel);
    let with =
        |label: &'static str, args: &'static [u8], badge: &'static str| WireCommand::SetItemState {
            item: "s0",
            flags: 0,
            label,
            label_args: args,
            badge,
            badge_args: &[],
        };
    let mut refused = |command: WireCommand<'_>, status: i32| {
        let (submitted, _) = run(&mut kernel, &[command], OK);
        assert_eq!(submitted, status, "{command:?}");
        assert_eq!(pushed(&kernel, "s0"), None, "nothing is kept");
    };
    // (Arguments without a key never get here: the wire refuses them.)
    // Bytes that are no argument batch.
    refused(with("fake.label", b"\xff\xff\xff", ""), ERR_INVALID);
    // A text no menu could show whole.
    refused(
        with("fake.long", b"", ""),
        paperpaw_native::host_glue::ERR_NO_SPACE,
    );
    refused(
        with("", b"", "fake.long"),
        paperpaw_native::host_glue::ERR_NO_SPACE,
    );
    // A key that resolves, with and without arguments.
    let (submitted, _) = run(&mut kernel, &[with("fake.plain", b"", "")], OK);
    assert_eq!(submitted, OK);
}

#[test]
fn a_plugin_holds_16_states_and_the_net_result_decides() {
    let mut kernel = kernel(17, true);
    let names: alloc::vec::Vec<alloc::string::String> =
        (0..17).map(|n| alloc::format!("s{n}")).collect();
    let all: alloc::vec::Vec<_> = names.iter().map(|n| state(n, HIDDEN)).collect();
    // A batch holds 8 commands: two of them fill the plugin's 16.
    let (submitted, _) = run(&mut kernel, &all[..8], OK);
    assert_eq!(submitted, OK);
    let (submitted, _) = run(&mut kernel, &all[8..16], OK);
    assert_eq!(submitted, OK);
    assert_eq!(pushed(&kernel, "s15"), Some(HIDDEN));
    // Full: a 17th is refused, a change of one is not, and a batch that
    // clears one and adds another is.
    let (submitted, _) = run(&mut kernel, &all[16..], OK);
    assert_eq!(submitted, ERR_FULL);
    let (submitted, _) = run(&mut kernel, &[state("s0", FAINT)], OK);
    assert_eq!(submitted, OK);
    let (submitted, _) = run(&mut kernel, &[state("s1", 0), state("s16", HIDDEN)], OK);
    assert_eq!(submitted, OK);
    assert_eq!(
        (pushed(&kernel, "s1"), pushed(&kernel, "s16")),
        (None, Some(HIDDEN))
    );
}

#[test]
fn a_host_without_the_store_refuses_the_record_as_unsupported() {
    let mut kernel = kernel(1, false);
    let (submitted, _) = run(&mut kernel, &[state("s0", HIDDEN)], OK);
    assert_eq!(submitted, ERR_UNSUPPORTED);
}

struct Recorder(alloc::rc::Rc<core::cell::RefCell<alloc::vec::Vec<u32>>>);
impl crate::native_host::event_producers::EventSink for Recorder {
    fn setting(&self, _: u64, _: &str, _: Result<&[u8], i32>) {}
    fn hint(&self, _: u32) {}
    fn registry(&self, what: u32) {
        self.0.borrow_mut().push(what);
    }
}

#[test]
fn a_changed_state_raises_registry_changed_bit_0_and_an_unchanged_one_does_not() {
    let mut kernel = kernel(3, true);
    let seen = alloc::rc::Rc::new(core::cell::RefCell::new(alloc::vec::Vec::new()));
    kernel
        .install_native_event_sink(Recorder(seen.clone()))
        .unwrap();
    // Two states in one batch: one event for the drain.
    let (submitted, _) = run(&mut kernel, &[state("s0", HIDDEN), state("s1", FAINT)], OK);
    assert_eq!(submitted, OK);
    assert_eq!(*seen.borrow(), [1]);
    // The same states again change nothing: no event.
    let (submitted, _) = run(&mut kernel, &[state("s0", HIDDEN), state("s1", FAINT)], OK);
    assert_eq!(submitted, OK);
    assert_eq!(seen.borrow().len(), 1);
    // A change, and the clearing of one, each raise it.
    let (submitted, _) = run(&mut kernel, &[state("s0", FAINT)], OK);
    assert_eq!((submitted, seen.borrow().len()), (OK, 2));
    let (submitted, _) = run(&mut kernel, &[state("s1", 0)], OK);
    assert_eq!((submitted, seen.borrow().len()), (OK, 3));
    // Clearing what was never set, and a callback that fails, do not.
    let (submitted, _) = run(&mut kernel, &[state("s2", 0)], OK);
    assert_eq!((submitted, seen.borrow().len()), (OK, 3));
    let (submitted, _) = run(&mut kernel, &[state("s2", HIDDEN)], ERR_INVALID);
    assert_eq!((submitted, seen.borrow().len()), (OK, 3));
    // A refused batch applies nothing and raises nothing.
    let (submitted, _) = run(&mut kernel, &[state("nobody", HIDDEN)], OK);
    assert_eq!((submitted, seen.borrow().len()), (ERR_NOT_FOUND, 3));
}

#[test]
fn the_room_is_back_after_every_kind_of_callback() {
    let mut kernel = kernel(2, true);
    let room = |kernel: &crate::Kernel| {
        kernel
            .shared
            .registry
            .native_declarations
            .as_ref()
            .unwrap()
            .item_room
            .get()
    };
    // Applied, a failed callback, a refused batch, a batch of defaults.
    for (commands, returned) in [
        (alloc::vec![state("s0", HIDDEN)], OK),
        (alloc::vec![state("s1", HIDDEN)], ERR_INVALID),
        (
            alloc::vec![state("s0", HIDDEN), state("nobody", HIDDEN)],
            OK,
        ),
        (alloc::vec![state("s0", 0)], OK),
    ] {
        let _ = run(&mut kernel, &commands, returned);
        assert_eq!(room(&kernel), 0, "{commands:?}");
    }
}
