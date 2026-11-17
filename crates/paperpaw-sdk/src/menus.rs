












/// What a menu entry needs to be offered (see
/// `Items::set_menu_need`), so
/// menus stay short on every device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuNeed {
    /// A device that speaks ([`Capabilities::speech`](crate::Capabilities)): hidden
    /// elsewhere, also where the user listed it.
    Speech,
    /// A device where PaperPaw shows the sleep screen
    /// ([`Capabilities::sleep`](crate::Capabilities)), hidden elsewhere.
    Sleep,
    /// A device whose screen PaperPaw can turn
    /// ([`settings::ROTATION`](crate::builtin::settings::ROTATION)),
    /// hidden elsewhere.
    Rotation,
    /// A device without a touch screen, where it is the way to do what a
    /// tap or a long press does elsewhere (e.g. choosing a link). On touch
    /// devices it is left out of the menus plugins define; users can still
    /// list it.
    NoTouch,
    /// A device with a touch screen, e.g. for what a tap or a swipe does:
    /// hidden elsewhere, also where the user listed it.
    Touch,
    /// A device with buttons in a row below the screen, whose row of
    /// hints shows what they do
    /// ([`settings::BUTTON_HINTS`](crate::builtin::settings::BUTTON_HINTS)),
    /// hidden elsewhere.
    ButtonRow,
    /// A device whose shell shows the screen in other colours than black
    /// and white ([`settings::TEXT_COLOR`](crate::builtin::settings::TEXT_COLOR)),
    /// hidden elsewhere.
    Colors,
    /// A device where PaperPaw manages Wi-Fi itself
    /// ([`Env::wifi`](crate::Env)), hidden elsewhere.
    Wifi,
    /// A device whose firmware PaperPaw is, updated by PaperPaw
    /// ([`Env::firmware`](crate::Env)), hidden elsewhere.
    Firmware,
    /// A device whose panel shows black and white alone
    /// ([`Capabilities::black_and_white`](crate::Capabilities)), hidden
    /// elsewhere.
    BlackAndWhite,
    /// A front light PaperPaw sets ([`Capabilities::frontlight`](crate::Capabilities)).
    Frontlight,
    /// A Bluetooth radio the kernel uses ([`Env::bluetooth`](crate::Env)),
    /// hidden elsewhere.
    Bluetooth,
    /// A device that can do a [`Capability`](crate::Capability) (new
    /// needs come as capabilities, without a new plugin interface).
    Has(crate::Capability),
}

impl MenuNeed {
    /// Whether a device that can do `caps` has what it needs (also for
    /// the kernel's `ChromeSpec::need`).
    pub fn met(self, caps: crate::Capabilities) -> bool {
        match self {
            MenuNeed::Speech => caps.speech,
            MenuNeed::Sleep => caps.sleep,
            MenuNeed::Rotation => caps.rotation,
            MenuNeed::Colors => caps.colors,
            MenuNeed::ButtonRow => caps.button_row,
            MenuNeed::Wifi => caps.wifi,
            MenuNeed::Bluetooth => caps.bluetooth,
            MenuNeed::Firmware => caps.firmware,
            MenuNeed::BlackAndWhite => caps.black_and_white,
            MenuNeed::NoTouch => !caps.touch,
            MenuNeed::Touch => caps.touch,
            MenuNeed::Frontlight => caps.frontlight,
            MenuNeed::Has(capability) => caps.has(capability),
        }
    }
}








#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn ids(menus: &Menus) -> Vec<&str> {
        menus.ids("m.menu").iter().map(|i| i.as_str()).collect()
    }

    fn entry(item: &'static str, order: i16) -> MenuEntry {
        MenuEntry::new(Id::new_static("m.menu"), Id::new_static(item)).order(order)
    }

    #[test]
    fn entries_go_where_they_are_placed() {
        let mut menus = Menus::new();
        menus.add(entry("m.b", 20));
        menus.add(entry("m.a", 10));
        let at = |item: &'static str, place: MenuPlace| {
            MenuEntry::new(Id::new_static("m.menu"), Id::new_static(item)).place(place)
        };
        // After one placed itself later; before one by order.
        menus.add(at("m.after-x", MenuPlace::After(Id::new_static("m.x"))));
        menus.add(at("m.x", MenuPlace::Before(Id::new_static("m.b"))));
        menus.add(at("m.first", MenuPlace::First));
        menus.add(at("m.last", MenuPlace::Last));
        menus.add(at("m.lost", MenuPlace::After(Id::new_static("m.nowhere"))));
        menus.add_section(
            MenuSection::new(Id::new_static("m.part"), "part", Id::new_static("m.menu")).order(15),
        );
        menus.add(at(
            "m.in-part",
            MenuPlace::InSection(Id::new_static("m.part")),
        ));
        assert_eq!(
            ids(&menus),
            // b (20) is in the part (15): last in it is after b.
            [
                "m.first",
                "m.a",
                "m.part",
                "m.x",
                "m.after-x",
                "m.b",
                "m.in-part",
                "m.last",
                "m.lost"
            ]
        );
        // Taken out by a plugin, which menu editors can tell.
        menus.hide(
            Id::new_static("m.menu"),
            Id::new_static("m.a"),
            Id::new_static("p"),
        );
        assert!(!ids(&menus).contains(&"m.a"));
        let by = menus.hidden_by("m.menu", "m.a");
        assert_eq!(by.map(Id::as_str), Some("p"));
        assert_eq!(menus.hidden_by("m.menu", "m.b"), None);
    }

    #[test]
    fn an_entry_placed_moves_the_same_one_by_order() {
        let mut menus = Menus::new();
        for (item, order) in [("m.a", 10), ("m.b", 20), ("m.c", 30)] {
            menus.add(entry(item, order));
        }
        // Another plugin puts c right after a: moved, not twice.
        menus.add(
            MenuEntry::new(Id::new_static("m.menu"), Id::new_static("m.c"))
                .place(MenuPlace::After(Id::new_static("m.a"))),
        );
        assert_eq!(ids(&menus), ["m.a", "m.c", "m.b"]);
    }

    #[test]
    fn several_in_one_place_keep_their_order() {
        let mut menus = Menus::new();
        menus.add(entry("m.a", 10));
        menus.add(entry("m.b", 20));
        let at = |item: &'static str, place: MenuPlace| {
            MenuEntry::new(Id::new_static("m.menu"), Id::new_static(item)).place(place)
        };
        let after_a = || MenuPlace::After(Id::new_static("m.a"));
        menus.add(at("m.first-1", MenuPlace::First));
        menus.add(at("m.after-1", after_a()));
        menus.add(at("m.first-2", MenuPlace::First));
        menus.add(at("m.after-2", after_a()));
        assert_eq!(
            ids(&menus),
            [
                "m.first-1",
                "m.first-2",
                "m.a",
                "m.after-1",
                "m.after-2",
                "m.b"
            ]
        );
    }

    #[test]
    fn customized_lists_the_users_menus_only() {
        let mut menus = Menus::new();
        menus.add(entry("m.a", 10));
        assert_eq!(menus.customized().count(), 0);
        menus.customize(Id::new_static("u.empty"), Vec::new());
        menus.customize(Id::new_static("m.menu"), vec![Id::new_static("m.a")]);
        let mut listed: Vec<&str> = menus.customized().map(|id| id.as_str()).collect();
        listed.sort_unstable();
        assert_eq!(listed, ["m.menu", "u.empty"]);
        menus.reset("u.empty");
        assert_eq!(menus.customized().count(), 1);
    }

    #[test]
    fn a_users_list_shows_entries_added_since_after_the_one_before() {
        let mut menus = Menus::new();
        for (item, order) in [("m.a", 10), ("m.b", 20), ("m.c", 30)] {
            menus.add(entry(item, order));
        }
        // Without b, c before a, knowing a, b and c.
        let menu = Id::new_static("m.menu");
        menus.customize(
            menu.clone(),
            vec![Id::new_static("m.c"), Id::new_static("m.a")],
        );
        // Known: what was there and left out (b).
        menus.set_known(menu.clone(), vec![Id::new_static("m.b")]);
        // A plugin installed later adds two: after a, and first.
        menus.add(entry("m.new", 15));
        menus.add(entry("m.top", 1));
        assert_eq!(ids(&menus), ["m.top", "m.c", "m.a", "m.new"]);
        // A list kept without knowing stays as it is.
        menus.known.remove("m.menu");
        assert_eq!(ids(&menus), ["m.c", "m.a"]);
    }

    #[test]
    fn orders_entries_and_honours_user_menus() {
        let mut menus = Menus::new();
        for (action, order) in [("a.second", 20), ("a.first", 10), ("a.untitled", 5)] {
            menus.add(MenuEntry {
                menu: Id::new_static("a.menu"),
                item: Id::parse(action).unwrap(),
                order,
                place: None,
            });
        }
        menus.set_title(Id::new_static("a.first"), Cow::Borrowed("First"));
        menus.set_title(Id::new_static("a.second"), Cow::Borrowed("Second"));
        fn titles(menus: &Menus) -> Vec<String> {
            let (actions, settings) = (crate::Actions::new(), crate::Settings::new());
            let view = MenuView::new(menus, &actions, &settings);
            view.items("a.menu")
                .iter()
                .map(|i| String::from(i.title))
                .collect()
        }
        assert_eq!(titles(&menus), ["First", "Second"]);

        menus.customize(Id::new_static("a.menu"), vec![Id::new_static("a.second")]);
        assert_eq!(titles(&menus), ["Second"]);
        let (actions, settings) = (crate::Actions::new(), crate::Settings::new());
        assert!(
            MenuView::new(&menus, &actions, &settings)
                .items("other")
                .is_empty()
        );

        // Entries for devices without touch, or that speak.
        menus.reset("a.menu");
        menus.add(MenuEntry {
            menu: Id::new_static("a.menu"),
            item: Id::new_static("a.links"),
            order: 30,
            place: None,
        });
        menus.set_title(Id::new_static("a.links"), Cow::Borrowed("Links"));
        menus.set_need(Id::new_static("a.links"), MenuNeed::NoTouch);
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Links"));
        menus.set_capabilities(crate::Capabilities {
            touch: true,
            ..Default::default()
        });
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Second"));
        // One for touch screens shows only on them.
        menus.set_need(Id::new_static("a.second"), MenuNeed::Touch);
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Second"));
        menus.set_capabilities(crate::Capabilities::default());
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Links"));
        menus.set_capabilities(crate::Capabilities {
            touch: true,
            ..Default::default()
        });
        // Listed by the user, it shows; one that needs speech does not.
        menus.set_need(Id::new_static("a.second"), MenuNeed::Speech);
        menus.customize(
            Id::new_static("a.menu"),
            alloc::vec![Id::new_static("a.links"), Id::new_static("a.second")],
        );
        assert_eq!(titles(&menus), ["Links"]);

        // Left out where a menu is opened.
        let params = crate::Params::new().with(crate::builtin::params::HIDE, "a.first, a.links");
        assert!(Menus::hidden(&params, "a.links"));
        assert!(!Menus::hidden(&params, "a.second"));
        assert!(!Menus::hidden(&crate::Params::new(), "a.links"));
    }
}
