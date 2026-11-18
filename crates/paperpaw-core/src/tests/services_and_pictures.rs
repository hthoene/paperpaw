//! Services between plugins, the row of hints, large pictures, the network and old settings files.

use super::*;

/// Publishes a service and a point (`provider`), or uses them (`user`).
pub(crate) struct ServicePlugin {
    id: &'static str,
    log: Log,
}

impl ServicePlugin {
    pub(crate) fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::parse(self.id).unwrap(),
            "Services",
            Version::new(1, 0, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }
}

pub(super) fn service_plugin(
    kept: ServicePlugin,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        if kept.id == "provider" {
            r.provide(
                Id::new_static("provider.greeting"),
                Greeting("hello"),
                Version::new(1, 0, 0),
            )?;
            // Not in its namespace: refused.
            let stolen = r.provide(
                Id::new_static("core.greeting"),
                Greeting("x"),
                Version::new(1, 0, 0),
            );
            assert!(matches!(stolen, Err(RegisterError::Namespace { .. })));
            return r.contribute(Id::new_static("provider.shapes"), Cow::Borrowed("circle"));
        }
        r.contribute(Id::new_static("provider.shapes"), Cow::Borrowed("square"))?;
        hear_into(&kept.log);
        r.subscribe(paperpaw_sdk::builtin::events::STARTED, |k, _| {
            k.with_env(|env, _| {
                let greeting = env
                    .services()
                    .get::<Greeting>("provider.greeting", Version::new(1, 0, 0))
                    .map(|g| &g.0);
                heard(alloc::format!("{greeting:?}"));
                for (plugin, shape) in env.services().contributions("provider.shapes") {
                    heard(alloc::format!("{plugin}:{shape}"));
                }
            })
        })
    }
}

#[test]
fn plugins_offer_each_other_services() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .service_plugin(ServicePlugin {
            id: "user",
            log: log.clone(),
        })
        .service_plugin(ServicePlugin {
            id: "provider",
            log: log.clone(),
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(
        *log.borrow(),
        ["Some(\"hello\")", "user:square", "provider:circle"]
    );
}

/// A screen of `sized`: the reader's shows the row of hints on a press.
struct SizedScreen {
    reader: bool,
}

impl crate::testing::Fake for SizedScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn hints(&self) -> Option<paperpaw_sdk::HintVisibility> {
        self.reader.then_some(paperpaw_sdk::HintVisibility::OnPress)
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }

    fn hint(&self, action: &Id) -> Option<paperpaw_sdk::Hint> {
        (*action == actions::CONFIRM).then(|| paperpaw_sdk::Hint::Text("Go".into()))
    }
}

/// Screens for the home and the reader slot that note the height they
/// were made for; the home one names its confirm button.
pub(crate) fn sized_plugin(
    kept: Rc<RefCell<Vec<(&'static str, u16)>>>,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        for (name, slot) in [("home", slots::HOME), ("reader", slots::READER)] {
            let made = kept.clone();
            let id = Id::parse(&alloc::format!("sized.{name}")).unwrap();
            r.add_fake(crate::testing::FakeSpec::new(
                id.clone(),
                "sized",
                Box::new(move |env, _| {
                    made.borrow_mut().push((name, env.device().display.height));
                    Box::new(SizedScreen {
                        reader: name == "reader",
                    })
                }),
            ))?;
            r.bind_slot(slot, id, Priority::DEFAULT)?;
        }
        Ok(())
    }
}

/// What [`sized_plugin`] registers under.
pub(crate) fn sized_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("sized"), "Sized", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

#[test]
fn a_row_of_hints_names_the_buttons_below_the_screen() {
    use paperpaw_sdk::Hint;
    let made = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .items(sized_plugin_manifest(), sized_plugin(made.clone()))
        .button_row(&["back", "confirm", "left", "right"])
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // The home screen is made for the part above the row.
    let (_, above) = made.borrow()[0];
    assert!(above < 90 && above > 45, "{above}");
    assert_eq!(kernel.hint_row(), Some(Rect::new(0, above, 90, 90 - above)));
    // Nothing to go back to; the screen's own word; arrows.
    assert_eq!(
        kernel.hint.shown,
        [
            Hint::Hidden,
            Hint::Text("Go".into()),
            Hint::Icon("chevron-left"),
            Hint::Icon("chevron-right"),
        ]
    );

    // The reader keeps all of the screen, as it says: made once, a plugin's
    // screen is laid out for the part it then has, not made again.
    kernel.apply(Ask::OpenSlot(slots::READER, paperpaw_sdk::Params::new()));
    kernel.finish();
    assert_eq!(made.borrow()[1..], [("reader", above)]);
    assert_eq!(kernel.stack.last().unwrap().area.height, 90);
    assert_eq!(kernel.hint_row(), None);
    // A button of the row shows the row over the page for a while; a side
    // button does not.
    press(&mut kernel, 50, "page-forward");
    assert_eq!(kernel.hint_row(), None);
    let effects = press(&mut kernel, 100, "confirm");
    let over = Rect::new(0, above, 90, 90 - above);
    assert_eq!(kernel.hint_row(), Some(over));
    assert!(effects.wake_at().is_some());
    render(&mut kernel);
    // Recognized as the button is let go, 50 ms after it went down.
    kernel.tick(Instant(150 + 4000));
    assert_eq!(kernel.hint_row(), None);
    // Gone, the page is drawn there again.
    assert_eq!(kernel.frame.unrendered.map(|r| r.bottom()), Some(90));
    kernel.apply(Ask::ShowHints);
    kernel.finish();
    assert_eq!(kernel.hint_row(), Some(over));
    // Back from it, the home screen has the part above the row again.
    press(&mut kernel, 5000, "back");
    assert_eq!(kernel.content().height, above);
    assert_eq!(kernel.hint.shown[0], Hint::Hidden);

    // Switched off: the screens are laid out anew for all of it.
    kernel.apply(Ask::set(
        &paperpaw_sdk::builtin::settings::BUTTON_HINTS,
        false,
    ));
    kernel.finish();
    assert_eq!(kernel.content().height, 90);
    assert_eq!(kernel.hint_row(), None);
}

struct PictureScreen(Rc<RefCell<bool>>);

impl crate::testing::Fake for PictureScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
        if *self.0.borrow() {
            let row = alloc::vec![128u8; usize::from(env.device().display.width)];
            for y in 0..i32::from(env.device().display.height) {
                canvas.draw_gray_row(0, y, &row);
            }
        } else {
            canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A home screen that shows a picture over all of it while asked to.
pub(crate) fn picture_plugin(
    kept: Rc<RefCell<bool>>,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let shown = kept.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("picture.home"),
            "picture",
            Box::new(move |_, _| Box::new(PictureScreen(shown.clone()))),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("picture.home"),
            Priority::DEFAULT,
        )
    }
}

/// What [`picture_plugin`] registers under.
pub(crate) fn picture_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("picture"), "Picture", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

#[test]
fn leaving_a_large_picture_refreshes_in_full() {
    let shown = Rc::new(RefCell::new(false));
    let mut kernel = Kernel::builder(DEVICE)
        .items(picture_plugin_manifest(), picture_plugin(shown.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let mode = |effects: crate::Effects| effects.refresh().map(|(_, mode)| mode);
    // A picture shown (drawn when rendered): asked as it is; leaving it,
    // in full.
    *shown.borrow_mut() = true;
    kernel.apply(Ask::Redraw(RefreshMode::Partial));
    let effects = kernel.finish();
    assert_eq!(mode(effects), Some(RefreshMode::Partial));
    render(&mut kernel);
    assert!(kernel.frame.pictures_shown);
    *shown.borrow_mut() = false;
    kernel.apply(Ask::Redraw(RefreshMode::Partial));
    assert_eq!(mode(kernel.finish()), Some(RefreshMode::Full));
    render(&mut kernel);
    assert!(!kernel.frame.pictures_shown);
}

/// Leaving a screen that showed a large picture for another is no full
/// refresh (Decision 212, #294: Back from a cover page to Home); turning on
/// the same screen is (`leaving_a_large_picture_refreshes_in_full`).
#[test]
fn leaving_a_screen_with_a_large_picture_refreshes_as_asked() {
    let shown = Rc::new(RefCell::new(true));
    let mut kernel = Kernel::builder(DEVICE)
        .items(picture_plugin_manifest(), picture_plugin(shown.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel.apply(Ask::Redraw(RefreshMode::Partial));
    kernel.finish();
    render(&mut kernel);
    assert!(kernel.frame.pictures_shown);
    // The pictures were shown by another opening of a screen than the one on top.
    kernel.frame.pictures_on = kernel
        .frame
        .pictures_on
        .map(|serial| serial.wrapping_add(1));
    *shown.borrow_mut() = false;
    kernel.apply(Ask::Redraw(RefreshMode::Partial));
    let effects = kernel.finish();
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Partial)
    );
}

/// Writes down the network's events, and asks for Wi-Fi on when the
/// network is wanted (as the `wifi` plugin does).
pub(crate) fn net_plugin(kept: Log) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        use paperpaw_sdk::builtin::events;
        hear_into(&kept);
        /// Writes down `name` and what it says.
        fn note(name: &str, params: &paperpaw_sdk::Params) {
            let extra = params
                .get(paperpaw_sdk::http_params::STATUS)
                .or(params.get(paperpaw_sdk::builtin::params::REASON))
                .unwrap_or_default();
            heard(alloc::format!("{name} {extra}"));
        }
        let hears: [(Id, crate::Hear); 6] = [
            (events::NETWORK_WANTED.into(), |k, p| {
                note(events::NETWORK_WANTED.id().as_str(), p);
                k.with_env(|_, out| out.push(Ask::Wifi(paperpaw_sdk::wifi::WifiCommand::On)));
            }),
            (events::NETWORK_IDLE.into(), |_, p| {
                note(events::NETWORK_IDLE.id().as_str(), p)
            }),
            (events::WIFI_CHANGED.into(), |_, p| {
                note(events::WIFI_CHANGED.id().as_str(), p)
            }),
            (events::WIFI_SCANNED.into(), |_, p| {
                note(events::WIFI_SCANNED.id().as_str(), p)
            }),
            (events::WIFI_FAILED.into(), |_, p| {
                note(events::WIFI_FAILED.id().as_str(), p)
            }),
            (Id::new_static("net.reply"), |_, p| note("net.reply", p)),
        ];
        for (event, run) in hears {
            r.subscribe(event, run)?;
        }
        Ok(())
    }
}

/// What [`net_plugin`] registers under.
pub(crate) fn net_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("net"), "Net", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NETWORK)
}

pub(super) fn wifi_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(net_plugin_manifest(), net_plugin(log.clone()))
        .wifi(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

pub(super) fn get(kernel: &mut Kernel) {
    kernel.request(paperpaw_sdk::HttpRequest::new(
        "GET",
        "http://example.org/",
        Id::new_static("net.reply"),
    ));
    kernel.finish();
}

/// `paperpaw.ini` as version 1 wrote it stays readable (see
/// `docs/data-files.md`).
#[test]
fn settings_files_of_version_1_are_read() {
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "paperpaw.ini",
        include_str!("../../testdata/paperpaw.ini.v1"),
    );
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(kernel.shared.menus.is_customized("core.main"));
    let main: Vec<&str> = kernel
        .shared
        .menus
        .ids("core.main")
        .into_iter()
        .map(Id::as_str)
        .collect();
    assert_eq!(main, ["core.toggle-night-mode", "core.back"]);
    assert!(kernel.take_warnings().is_empty());
}

/// A book's own settings (`settings.ini` in its folder) as version 1
/// wrote them stay readable (see `docs/data-files.md`).
#[test]
fn book_settings_of_version_1_are_read() {
    let path = ".paperpaw/plugins/reader/books/0000000000000001/settings.ini";
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with(path, include_str!("../../testdata/book-settings.ini.v1"));
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    kernel.set_scope(Some(String::from(path)));
    let settings = &kernel.shared.settings;
    assert_eq!(
        settings.scoped_value(settings::SCOPE_ONLY.as_str()),
        Some(&SettingValue::Bool(true))
    );
    assert_eq!(
        settings.scoped_value(settings::READING_FONT.as_str()),
        Some(&SettingValue::Text(Cow::Borrowed(settings::DEFAULT_FONT)))
    );
}

/// The shell takes everything at once: the radio first, network work only
/// once there is a network.
#[test]
fn the_shell_takes_all_it_is_asked_at_once() {
    let log = log();
    let mut kernel = wifi_kernel(&log);
    kernel.request(paperpaw_sdk::HttpRequest::new(
        "GET",
        "http://example.org/",
        Id::new_static("net.reply"),
    ));
    kernel.finish();
    assert_eq!(
        kernel.take_io(),
        [Io::Wifi(paperpaw_sdk::wifi::WifiCommand::On)]
    );
    let online = paperpaw_sdk::wifi::WifiState::Online {
        ssid: String::from("Home"),
        address: String::from("10.0.0.2"),
        name: None,
    };
    kernel.wifi_event(Instant(10), crate::WifiEvent::State(online));
    let io = kernel.take_io();
    assert!(matches!(io.as_slice(), [Io::Http(call)] if call.url == "http://example.org/"));
    assert_eq!(kernel.take_io(), []);
}

#[test]
fn overlays_leave_the_rows_they_do_not_cover() {
    let uncovered = crate::uncovered;
    let part = Rect::new(0, 260, 480, 100);
    // A menu from 270 down: the ten rows above it.
    let menu = Rect::new(0, 270, 480, 530);
    assert_eq!(uncovered(part, menu), Some(Rect::new(0, 260, 480, 10)));
    // A bar at the top down to 300: the rows below it.
    let bar = Rect::new(0, 0, 480, 300);
    assert_eq!(uncovered(part, bar), Some(Rect::new(0, 300, 480, 60)));
    // All of it covered; none of it; a narrower one: all.
    assert_eq!(uncovered(part, Rect::new(0, 200, 480, 600)), None);
    assert_eq!(uncovered(part, Rect::new(0, 500, 480, 100)), Some(part));
    assert_eq!(uncovered(part, Rect::new(40, 200, 400, 600)), Some(part));
}
