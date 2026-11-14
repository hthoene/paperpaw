//! Which build of a plugin fits this device: what a build says it needs of
//! the firmware against what the firmware provides, the `abi` a build names
//! (a fingerprint of format 4 or `<major>.<minor>` of format 5) and the
//! address the library is asked at (the interface-6 `needs.rs` and catalogue
//! tests).
#![allow(clippy::unwrap_used, reason = "fixtures")]
use paperpaw_plugin_store::native_item::{Device, Item};
use paperpaw_plugin_store::native_model::{
    Abi, FAMILIES, Fit, Needs, OFFICIAL_LIST, Provides, list_url, plugin_url,
};
use paperpaw_sdk::native::{Fingerprint, INTERFACE};

fn provides() -> Provides {
    Provides {
        minor: Some(2),
        api: 400,
        families: [3, 2, 0, 0, 0, 0, 0, 0],
        oldest: [2, 1, 0, 0, 0, 0, 0, 0],
        checks_families: true,
    }
}

fn needing(api: u32, families: [u16; FAMILIES]) -> Needs {
    Needs {
        interface: Some(INTERFACE),
        minor: None,
        api: Some(api),
        families: Some(families),
    }
}

#[test]
fn a_build_that_says_nothing_fits_as_before() {
    assert_eq!(provides().fit(&Needs::default()), Fit::Yes);
    // An odd list of families is not read; the rest still counts.
    let odd = Needs {
        families: None,
        api: Some(401),
        ..Needs::default()
    };
    assert_eq!(provides().fit(&odd), Fit::Newer);
    // A firmware that does not check families (the X4 Pro's chip) is not held
    // back by those a build names.
    let pro = Provides {
        checks_families: false,
        ..provides()
    };
    assert_eq!(pro.fit(&needing(400, [9, 9, 9, 9, 9, 9, 9, 9])), Fit::Yes);
}

#[test]
fn what_the_firmware_lacks_is_a_newer_paperpaw() {
    let p = provides();
    assert_eq!(p.fit(&needing(400, [3, 2, 0, 0, 0, 0, 0, 0])), Fit::Yes);
    assert_eq!(p.fit(&needing(0, [2, 0, 0, 0, 0, 0, 0, 0])), Fit::Yes);
    // A table bigger than the firmware's; a family revision past its; a
    // family it does not have.
    assert_eq!(p.fit(&needing(404, [0; 8])), Fit::Newer);
    assert_eq!(p.fit(&needing(400, [4, 0, 0, 0, 0, 0, 0, 0])), Fit::Newer);
    assert_eq!(p.fit(&needing(400, [0, 0, 1, 0, 0, 0, 0, 0])), Fit::Newer);
    // A later interface, and a newer minor than the firmware speaks.
    let later = Needs {
        interface: Some(INTERFACE + 1),
        ..Needs::default()
    };
    assert_eq!(p.fit(&later), Fit::Newer);
    let minor = |minor| Needs {
        minor: Some(minor),
        ..Needs::default()
    };
    assert_eq!(p.fit(&minor(2)), Fit::Yes);
    assert_eq!(p.fit(&minor(3)), Fit::Newer);
    // Where the firmware's minor is not known, none is judged.
    let unknown = Provides { minor: None, ..p };
    assert_eq!(unknown.fit(&minor(9)), Fit::Yes);
}

#[test]
fn what_was_built_before_the_oldest_still_run_is_not_offered() {
    let p = provides();
    assert_eq!(p.fit(&needing(0, [1, 0, 0, 0, 0, 0, 0, 0])), Fit::Older);
    let before = Needs {
        interface: Some(INTERFACE - 1),
        ..Needs::default()
    };
    assert_eq!(p.fit(&before), Fit::Older);
    // Newer wins where both hold.
    assert_eq!(p.fit(&needing(500, [1, 0, 0, 0, 0, 0, 0, 0])), Fit::Newer);
}

#[test]
fn a_dotted_abi_is_judged_by_major_and_minor() {
    let device = Some(Fingerprint::of(INTERFACE, 0x1234));
    let dotted = Abi::parse("7.3").unwrap();
    assert_eq!((dotted.interface(), dotted.minor()), (7, Some(3)));
    // It has no draft or chip of its own: every device takes it as its own,
    // and the fit judges it.
    assert!(dotted.is_for(device) && dotted.is_for(None));
    // A fingerprint is the device's own or not.
    let print = Abi::parse(&format!("{:016x}", Fingerprint::of(INTERFACE, 0x1234).0)).unwrap();
    assert!(print.is_for(device));
    assert!(!print.is_for(Some(Fingerprint::of(INTERFACE, 0x9999))));
    assert!(!print.is_for(None));
    assert_eq!(print.minor(), None);
    // Anything else is none.
    for odd in [
        "", "7", "7.", ".3", "07.3", "7.03", "7.3.1", "+7.3", "7.-3", "7.65536", "x.y",
    ] {
        assert_eq!(Abi::parse(odd), None, "{odd:?}");
    }
    assert!(Abi::parse(" 7.0 ").is_some());
}

/// A device with `provides`; its fingerprint is the format 4 build's own.
fn device(provides: Option<Provides>) -> Device<'static> {
    Device {
        name: "x4",
        fingerprint: Some(Fingerprint::of(INTERFACE, 0x1234)),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides,
    }
}

/// A plugin of the library whose one build says `abi` (and `needs_api`).
fn with_build(abi: &str, needs_api: &str, device: &Device<'_>) -> Item {
    let mut item = Item::new();
    let mut read = |key: &[&str], value: &str| item.read(key, value, "x4").unwrap();
    read(&["id"], "clock");
    read(&["name"], "Clock");
    read(&["kind"], "code");
    read(&["latest", "version"], "1.0.0");
    read(&["latest", "builds", "[]", "device"], "x4");
    read(&["latest", "builds", "[]", "abi"], abi);
    read(&["latest", "builds", "[]", "needs_api"], needs_api);
    read(&["latest", "builds", "[]", "url"], "clock/code.ppn");
    item.build_done(device).unwrap();
    item
}

#[test]
fn a_build_that_needs_a_newer_paperpaw_is_listed_as_such_and_not_installed() {
    let device = device(Some(provides()));
    let item = with_build("7.2", "404", &device);
    assert!(item.code.is_none() && item.needs_newer);
    let entry = item.finish(&device);
    assert!(entry.code && entry.needs_newer);
    // The same build on a firmware that has the table: it is the plugin's code.
    let roomy = Provides {
        api: 404,
        ..provides()
    };
    let device = self::device(Some(roomy));
    let item = with_build("7.2", "404", &device);
    assert!(item.code.is_some() && !item.needs_newer);
}

#[test]
fn a_build_for_an_older_interface_or_another_device_is_neither_installed_nor_waited_for() {
    let device = device(Some(provides()));
    let older = format!("{}.0", INTERFACE - 1);
    let item = with_build(&older, "0", &device);
    assert!(item.code.is_none() && !item.needs_newer);
    // A fingerprint of another device is not for this one.
    let other = format!("{:016x}", Fingerprint::of(INTERFACE, 0x9999).0);
    let item = with_build(&other, "0", &device);
    assert!(item.code.is_none() && !item.needs_newer);
}

#[test]
fn a_plugin_the_library_has_no_build_of_for_this_paperpaw_is_listed_waiting() {
    let device = device(Some(provides()));
    for (waits, author) in [("author", true), ("update", false)] {
        let mut item = Item::new();
        item.read(&["id"], "clock", "x4").unwrap();
        item.read(&["name"], "Clock", "x4").unwrap();
        item.read(&["kind"], "code", "x4").unwrap();
        item.read(&["latest", "version"], "1.0.0", "x4").unwrap();
        item.read(&["waits"], waits, "x4").unwrap();
        assert!(item.fits(&device), "{waits}: listed");
        let entry = item.finish(&device);
        assert!(entry.code, "{waits}");
        assert_eq!(entry.waits_author, author, "{waits}");
        assert_eq!(entry.needs_newer, !author, "{waits}");
    }
    // Without a word of waiting and without a build it is not listed.
    let mut item = Item::new();
    item.read(&["id"], "clock", "x4").unwrap();
    item.read(&["name"], "Clock", "x4").unwrap();
    item.read(&["kind"], "code", "x4").unwrap();
    item.read(&["latest", "version"], "1.0.0", "x4").unwrap();
    assert!(!item.fits(&device));
}

#[test]
fn addresses_name_the_build_the_device_runs() {
    let abi = "00000000075bcd15";
    let list = list_url(OFFICIAL_LIST, "x4", "en", false, Some(abi)).unwrap();
    assert!(
        list.ends_with(&format!("device=x4&lang=en&abi={abi}")),
        "{list}"
    );
    let plain = list_url(OFFICIAL_LIST, "x4", "en", false, None).unwrap();
    assert!(!plain.contains("abi="), "{plain}");
    let page = plugin_url(OFFICIAL_LIST, "x4", "en", "clock", false, Some(abi)).unwrap();
    assert!(page.contains("plugins/clock.json?"), "{page}");
    assert!(page.contains(&format!("&abi={abi}")), "{page}");
    assert!(
        !plugin_url(OFFICIAL_LIST, "x4", "en", "clock", false, None)
            .unwrap()
            .contains("abi=")
    );
}
